use alloy::providers::Provider;
use angstrom_types::{
    primitive::PoolId,
    sol_bindings::{grouped_orders::GroupedVanillaOrder, rpc_orders::TopOfBlockOrder}
};
use rand::Rng;
use rand_distr::{Distribution, Normal};
use uniswap_v4::uniswap::pool_manager::SyncedUniswapPools;

mod order_builder;
use order_builder::OrderBuilder;

/// Order Generator is used for generating orders based off of
/// the current pool state.
///
/// Currently the way this is built is for every block, a true price
/// will be chosen based off of a sample of a normal distribution.
/// We will then generate orders around this sample point and stream
/// them out of the order generator.
pub struct OrderGenerator {
    provider:           Box<dyn Provider>,
    block_number:       u64,
    cur_price:          f64,
    price_distribution: PriceDistribution,
    builder:            OrderBuilder
}

impl OrderGenerator {
    pub fn new(
        provider: Box<dyn Provider>,
        pool_id: PoolId,
        pool_data: SyncedUniswapPools,
        block_number: u64
    ) -> Self {
        let price = pool_data
            .get(&pool_id)
            .unwrap()
            .read()
            .unwrap()
            .calculate_price();

        // bounds of 50% from start with a std of 3%
        let mut price_distribution =
            PriceDistribution::new(price, price * 1.5, price * 0.5, price * 0.03);
        let cur_price = price_distribution.generate_price();
        let builder = OrderBuilder::new(pool_id, pool_data);

        Self { provider, block_number, price_distribution, cur_price, builder }
    }

    /// updates the block number and samples a new true price.
    pub fn new_block(&mut self, block: u64) {
        self.block_number = block;

        let cur_price = self.price_distribution.generate_price();
        self.cur_price = cur_price;
    }

    pub fn generate_set<const O: usize>(&self, partial_pct: f64) -> GeneratedPoolOrders<O> {
        // tob always goes to true price
        let tob = self
            .builder
            .build_tob_order(self.cur_price, self.block_number + 1);

        let price_samples: [f64; O] = self.price_distribution.sample_around_price();

        let mut rng = rand::thread_rng();
        let book = core::array::from_fn(|i| {
            let price = price_samples[i];
            self.builder
                .build_user_order(price, self.block_number + 1, rng.gen_bool(partial_pct))
        });

        GeneratedPoolOrders { tob, book }
    }
}

/// container for orders generated for a specific pool
pub struct GeneratedPoolOrders<const N: usize> {
    pub tob:  TopOfBlockOrder,
    pub book: [GroupedVanillaOrder; N]
}

/// samples from a normal price distribution where true price is a
/// average of last N prices.
pub struct PriceDistribution<const N: usize = 10> {
    last_prices: [f64; N],
    upper_bound: f64,
    lower_bound: f64,
    sd_factor:   f64
}

impl<const N: usize> PriceDistribution<N> {
    pub fn new(start_price: f64, upper_bound: f64, lower_bound: f64, sd_factor: f64) -> Self {
        let last_prices = [start_price; N];

        Self { last_prices, upper_bound, lower_bound, sd_factor }
    }

    /// samples around mean price
    pub fn sample_around_price<const O: usize>(&self) -> [f64; O] {
        let price_avg = self.last_prices.iter().sum::<f64>() / N as f64;
        let normal = Normal::new(price_avg, price_avg / self.sd_factor).unwrap();
        let mut rng = rand::thread_rng();

        core::array::from_fn(|_| {
            normal
                .sample(&mut rng)
                .clamp(self.lower_bound, self.upper_bound)
        })
    }

    /// updates the mean price
    pub fn generate_price(&mut self) -> f64 {
        let price_avg = self.last_prices.iter().sum::<f64>() / N as f64;
        let normal = Normal::new(price_avg, price_avg / self.sd_factor).unwrap();
        let mut rng = rand::thread_rng();

        let new_price = normal
            .sample(&mut rng)
            .clamp(self.lower_bound, self.upper_bound);

        // move last entry to front
        self.last_prices.rotate_right(1);
        // overwrite front entry
        self.last_prices[0] = new_price;

        new_price
    }
}
