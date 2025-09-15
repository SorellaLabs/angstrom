use angstrom_rpc::api::OrderApiClient;
use angstrom_types::primitive::PoolId;
use uniswap_v4::uniswap::pool_manager::SyncedUniswapPool;

use super::{
    GeneratedPoolOrders, InternalBalanceMode, PriceDistribution, order_builder::OrderBuilder
};
/// Order Generator is used for generating orders based off of
/// the current pool state.
///
/// Currently the way this is built is for every block, a true price
/// will be chosen based off of a sample of a normal distribution.
/// We will then generate orders around this sample point and stream
/// them out of the order generator.
pub struct PoolOrderGenerator<T: OrderApiClient> {
    block_number:       u64,
    cur_price:          f64,
    price_distribution: PriceDistribution,
    builder:            OrderBuilder,
    pub pool_id:        PoolId,
    client:             Option<T>
}

impl<T: OrderApiClient + Clone> PoolOrderGenerator<T> {
    pub fn new(
        pool_id: PoolId,
        pool_data: SyncedUniswapPool,
        block_number: u64,
        client: T,
        internal_balance_mode: InternalBalanceMode
    ) -> Self {
        let price = pool_data.read().unwrap().calculate_price();

        // bounds of 50% from start with a std of 5%
        let mut price_distribution = PriceDistribution::new(price, f64::MAX - 1.0, 0.0, 25.0);
        let cur_price = price_distribution.generate_price();
        let builder = OrderBuilder::new(pool_data, internal_balance_mode.clone());

        Self { block_number, price_distribution, cur_price, builder, pool_id, client: Some(client) }
    }

    pub fn new_with_cfg_distro(
        pool_id: PoolId,
        pool_data: SyncedUniswapPool,
        block_number: u64,
        sd_pct: f64,
        internal_balance_mode: InternalBalanceMode
    ) -> Self {
        let price = pool_data.read().unwrap().calculate_price();

        // bounds of 50% from start with a std of 2%
        let mut price_distribution = PriceDistribution::new(price, f64::MAX - 1.0, 0.0, sd_pct);
        let cur_price = price_distribution.generate_price();
        let builder = OrderBuilder::new(pool_data, internal_balance_mode.clone());

        Self { block_number, price_distribution, cur_price, builder, pool_id, client: None }
    }

    /// updates the block number and samples a new true price.
    pub fn new_block(&mut self, block: u64) {
        self.block_number = block;

        let cur_price = self.price_distribution.generate_price();
        self.cur_price = cur_price;
    }

    pub async fn generate_set(&self, amount: usize, partial_pct: f64) -> GeneratedPoolOrders {
        let (t0, t1) = self.builder.get_token0_token1();
        let gas_book = if let Some(client) = self.client.as_ref() {
            client
                .estimate_gas(true, false, t0, t1)
                .await
                .unwrap()
                .unwrap()
                .0
                .to::<u128>()
        } else {
            0
        };

        let gas_tob = if let Some(client) = self.client.as_ref() {
            client
                .estimate_gas(false, false, t0, t1)
                .await
                .unwrap()
                .unwrap()
                .0
                .to::<u128>()
        } else {
            0
        };

        let tob = self
            .builder
            .build_tob_order(self.cur_price, self.block_number + 1, gas_tob);

        let price_samples = self.price_distribution.sample_around_price(amount);
        let mut book = vec![];

        for price in price_samples.into_iter().take(amount) {
            book.push(self.builder.build_user_order(
                price,
                self.block_number + 1,
                partial_pct,
                gas_book
            ));
        }

        GeneratedPoolOrders { tob, book, pool_id: self.pool_id }
    }
}
