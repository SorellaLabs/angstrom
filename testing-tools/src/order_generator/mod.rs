use std::{collections::VecDeque, task::Poll};

use alloy::providers::Provider;
use angstrom_types::primitive::PoolId;
use futures::Stream;
use rand_distr::{Distribution, Normal};
use uniswap_v4::uniswap::pool_manager::SyncedUniswapPools;

/// Order Generator is used for generating orders based off of
/// the current pool state.
///
/// Currently the way this is built is for every block, a true price
/// will be chosen based off of a sample of a normal distribution.
/// We will then generate orders around this sample point and stream
/// them out of the order generator.
pub struct OrderGenerator {
    provider:           Box<dyn Provider>,
    /// specific pool id wanted
    pool_id:            PoolId,
    /// pools to based orders off of
    pool_data:          SyncedUniswapPools,
    block_number:       u64,
    price_distribution: PriceDistribution
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

        /// bounds of 50% from start with a std of 3%
        let price_distribution =
            PriceDistribution::new(price, price * 1.5, price * 0.5, price * 0.03);

        Self { provider, pool_id, pool_data, block_number, price_distribution }
    }

    // pub fn new_block
}

impl Stream for OrderGenerator {
    type Item = ();

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>
    ) -> std::task::Poll<Option<Self::Item>> {
        Poll::Pending
    }
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
