use angstrom_types::primitive::PoolId;
use uniswap_v4::uniswap::pool_manager::SyncedUniswapPool;

use super::{order_builder::OrderBuilder, GeneratedPoolOrders, PriceDistribution};
/// Order Generator is used for generating orders based off of
/// the current pool state.
///
/// Currently the way this is built is for every block, a true price
/// will be chosen based off of a sample of a normal distribution.
/// We will then generate orders around this sample point and stream
/// them out of the order generator.
pub struct PoolOrderGenerator {
    block_number:       u64,
    cur_price:          f64,
    price_distribution: PriceDistribution,
    builder:            OrderBuilder,
    pool_id:            PoolId
}

impl PoolOrderGenerator {
    pub fn new(pool_id: PoolId, pool_data: SyncedUniswapPool, block_number: u64) -> Self {
        let price = pool_data.read().unwrap().calculate_price();

        // bounds of 50% from start with a std of 10%
        let mut price_distribution =
            PriceDistribution::new(price, f64::INFINITY, f64::NEG_INFINITY, 5.0);
        let cur_price = price_distribution.generate_price();
        let builder = OrderBuilder::new(pool_data);

        Self { block_number, price_distribution, cur_price, builder, pool_id }
    }

    /// updates the block number and samples a new true price.
    pub fn new_block(&mut self, block: u64) {
        self.block_number = block;

        let cur_price = self.price_distribution.generate_price();
        self.cur_price = cur_price;
    }

    pub fn generate_set(&self, amount: usize, partial_pct: f64) -> GeneratedPoolOrders {
        let tob = self
            .builder
            .build_tob_order(self.cur_price, self.block_number + 1);

        let price_samples = self.price_distribution.sample_around_price(amount);
        let mut book = vec![];

        for price in price_samples.into_iter().take(amount) {
            book.push(
                self.builder
                    .build_user_order(price, self.block_number + 1, partial_pct)
            );
        }

        GeneratedPoolOrders { tob, book, pool_id: self.pool_id }
    }
}
