use std::ops::Range;

use angstrom_rpc::api::OrderApiClient;
use angstrom_types::{
    primitive::PoolId,
    sol_bindings::{grouped_orders::AllOrders, rpc_orders::TopOfBlockOrder}
};
use futures::stream::StreamExt;
use itertools::Itertools;
use rand::Rng;
use rand_distr::{Distribution, Normal};
use uniswap_v4::uniswap::pool_manager::SyncedUniswapPools;

mod order_builder;
mod pool_order_generator;
pub use pool_order_generator::PoolOrderGenerator;

/// Configuration for internal balance usage in generated orders
#[derive(Debug, Clone)]
pub enum InternalBalanceMode {
    /// All orders use internal balances
    Always,
    /// No orders use internal balances
    Never,
    /// Random percentage of orders use internal balances (0.0 to 1.0)
    Random(f64)
}

pub struct OrderGenerator<T: OrderApiClient> {
    pools:             Vec<PoolOrderGenerator<T>>,
    /// lower and upper bounds for the amount of book orders to generate
    order_amt_range:   Range<usize>,
    partial_pct_range: Range<f64>
}

impl<T: OrderApiClient + Clone> OrderGenerator<T> {
    pub fn new(
        pool_data: SyncedUniswapPools,
        block_number: u64,
        client: T,
        order_amt_range: Range<usize>,
        partial_pct_range: Range<f64>,
        internal_balance_mode: InternalBalanceMode
    ) -> Self {
        let pools = pool_data
            .iter()
            .map(|item| {
                let pool_id = item.key();
                let pool_data = item.value();
                PoolOrderGenerator::new(
                    *pool_id,
                    pool_data.clone(),
                    block_number,
                    client.clone(),
                    internal_balance_mode.clone()
                )
            })
            .sorted_by_key(|k| k.pool_id)
            .collect::<Vec<_>>();

        Self { pools, order_amt_range, partial_pct_range }
    }

    pub fn new_block(&mut self, block_number: u64) {
        self.pools
            .iter_mut()
            .for_each(|pool| pool.new_block(block_number));
    }

    pub fn generate_orders(&self) -> impl Future<Output = Vec<GeneratedPoolOrders>> + '_ {
        let mut rng = rand::rng();
        let first = rng.random_range(self.order_amt_range.clone());
        let second = rng.random_range(self.partial_pct_range.clone());

        futures::stream::iter(self.pools.iter())
            .then(move |pool| async move { pool.generate_set(first, second).await })
            .collect()
    }
}

/// container for orders generated for a specific pool
pub struct GeneratedPoolOrders {
    pub pool_id: PoolId,
    pub tob:     TopOfBlockOrder,
    pub book:    Vec<AllOrders>
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
        let mut rng = rand::rng();

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
        let normal = Normal::new(price_avg, price_avg * (self.sd_factor / 100.0)).unwrap();
        let mut rng = rand::rng();

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
