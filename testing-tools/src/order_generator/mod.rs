use std::ops::Range;

use angstrom_types::{
    primitive::PoolId,
    sol_bindings::{grouped_orders::GroupedVanillaOrder, rpc_orders::TopOfBlockOrder}
};
use rand::Rng;
use rand_distr::{Distribution, Normal};
use uniswap_v4::uniswap::pool_manager::SyncedUniswapPools;

mod order_builder;
mod pool_order_generator;
use pool_order_generator::PoolOrderGenerator;

pub struct OrderGenerator {
    pools:             Vec<PoolOrderGenerator>,
    /// lower and upper bounds for the amount of book orders to generate
    order_amt_range:   Range<usize>,
    partial_pct_range: Range<f64>
}

impl OrderGenerator {
    pub fn new(
        pool_data: SyncedUniswapPools,
        block_number: u64,
        order_amt_range: Range<usize>,
        partial_pct_range: Range<f64>
    ) -> Self {
        let pools = pool_data
            .iter()
            .map(|(pool_id, pool_data)| {
                PoolOrderGenerator::new(*pool_id, pool_data.clone(), block_number)
            })
            .collect::<Vec<_>>();

        Self { pools, order_amt_range, partial_pct_range }
    }

    pub fn new_block(&mut self, block_number: u64) {
        self.pools
            .iter_mut()
            .for_each(|pool| pool.new_block(block_number));
    }

    pub fn generate_orders(&self) -> Vec<GeneratedPoolOrders> {
        let mut rng = rand::thread_rng();
        self.pools
            .iter()
            .map(|pool| {
                pool.generate_set(
                    rng.gen_range(self.order_amt_range.clone()),
                    rng.gen_range(self.partial_pct_range.clone())
                )
            })
            .collect::<Vec<_>>()
    }
}

/// container for orders generated for a specific pool
pub struct GeneratedPoolOrders {
    pub pool_id: PoolId,
    pub tob:     TopOfBlockOrder,
    pub book:    Vec<GroupedVanillaOrder>
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
    pub fn sample_around_price(&self, amount: usize) -> Vec<f64> {
        let price_avg = self.last_prices.iter().sum::<f64>() / N as f64;
        let normal = Normal::new(price_avg, price_avg * (self.sd_factor / 100.0)).unwrap();
        let mut rng = rand::thread_rng();

        let mut res = Vec::with_capacity(amount);
        for _ in 0..amount {
            res.push(
                normal
                    .sample(&mut rng)
                    .clamp(self.lower_bound, self.upper_bound)
            );
        }
        res
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
