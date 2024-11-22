use alloy::primitives::I256;
use alloy_primitives::{Address, BigIntConversionError, ParseSignedError, U256};
use angstrom_types::{block_sync::BlockSyncConsumer, sol_bindings::rpc_orders::TopOfBlockOrder};
use rand::Rng;
use rand_distr::{Distribution, Exp, Normal};
use thiserror::Error;
use uniswap_v4::uniswap::{
    pool::{EnhancedUniswapPool, PoolError, SwapSimulationError},
    pool_data_loader::PoolDataLoader,
    pool_manager::UniswapPoolManager,
    pool_providers::PoolManagerProvider
};

pub struct OrderGeneratorConfig {
    min_amount:     f64,
    max_amount:     f64,
    std_dev_orders: f64
}

impl Default for OrderGeneratorConfig {
    fn default() -> Self {
        Self { min_amount: 0.1, max_amount: 100.0, std_dev_orders: 1.0 }
    }
}

struct PriceDistribution {
    exp:        Exp<f64>,
    min_amount: f64,
    max_amount: f64
}

impl PriceDistribution {
    fn new(rate: f64, min_amount: f64, max_amount: f64) -> Result<Self, rand_distr::ExpError> {
        Ok(PriceDistribution { exp: Exp::new(rate)?, min_amount, max_amount })
    }
}

impl Distribution<f64> for PriceDistribution {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> f64 {
        let mut value = self.exp.sample(rng);
        value = value * (self.max_amount - self.min_amount) / 10.0 + self.min_amount;
        value.max(self.min_amount).min(self.max_amount)
    }
}

#[allow(dead_code)]
pub struct UserOrderGenerator<P, BlockSync, Loader>
where
    Loader: PoolDataLoader<Address> + Default
{
    pool_manager:             UniswapPoolManager<P, BlockSync, Loader, Address>,
    config:                   OrderGeneratorConfig,
    price_distribution:       PriceDistribution,
    order_count_distribution: Normal<f64>,
    pool_address:             Address
}

impl<P, BlockSync, Loader> UserOrderGenerator<P, BlockSync, Loader>
where
    P: PoolManagerProvider + Send + Sync + 'static,
    BlockSync: BlockSyncConsumer + Send + Sync + 'static,
    Loader: PoolDataLoader<Address> + Default + Send + Sync + 'static
{
    pub fn new(
        pool_address: Address,
        pool_manager: UniswapPoolManager<P, BlockSync, Loader, Address>,
        config: Option<OrderGeneratorConfig>
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let config = config.unwrap_or_default();
        // 0.8 seems like what we are looking for
        let price_distribution = PriceDistribution::new(0.8, config.min_amount, config.max_amount)?;

        let order_count_distribution = Normal::new(0.0, config.std_dev_orders)?;
        Ok(Self {
            pool_address,
            pool_manager,
            config,
            price_distribution,
            order_count_distribution
        })
    }

    pub async fn monitor(&self) {
        let (mut pool_update_rx, _join_handles) =
            match self.pool_manager.subscribe_state_changes().await {
                Ok(result) => result,
                Err(e) => {
                    tracing::error!("Failed to subscribe to state changes: {}", e);
                    return;
                }
            };

        loop {
            if let Some((address, _)) = pool_update_rx.recv().await {
                if let Some(pool) = self.pool_manager.pool(&address) {
                    let _ = self.generate_orders(&*pool);
                }
            }
        }
    }

    pub async fn next_orders(&self) -> Vec<TopOfBlockOrder> {
        let (mut pool_update_rx, _) = self.pool_manager.subscribe_state_changes().await.unwrap();

        if let Some((address, _)) = pool_update_rx.recv().await {
            if let Some(pool) = self.pool_manager.pool(&address) {
                return self.generate_orders(&*pool);
            }
        }
        Vec::new()
    }

    fn generate_orders(&self, pool: &EnhancedUniswapPool<Loader>) -> Vec<TopOfBlockOrder> {
        let mut rng = rand::thread_rng();
        let raw_sample = self.order_count_distribution.sample(&mut rng);
        let shifted_sample = raw_sample.abs(); // Make positive
        let fat_tailed_sample = shifted_sample.powf(2.0); // Increase tail heaviness
        let num_orders = (fat_tailed_sample.round() as u32).min(20); // Cap at 20 orders

        (0..num_orders)
            .map(|_| self.generate_order(pool, &mut rng).unwrap())
            .collect()
    }

    fn generate_order(
        &self,
        pool: &EnhancedUniswapPool<Loader>,
        rng: &mut impl Rng
    ) -> Result<TopOfBlockOrder, UserOrderError> {
        let amount = self.price_distribution.sample(rng);
        let zero_for_one = rng.gen_bool(0.5);

        let token_in = if zero_for_one { pool.token_a } else { pool.token_b };
        let amount_in = if zero_for_one {
            let pool_price = pool.exchange_price(None)?;

            // Convert amount from ETH to USDC using the pool price
            let amount_in_usdc = amount * pool_price;

            // Convert amount to token A (USDC) decimals
            let amount = amount_in_usdc * 10_f64.powi(pool.token_a_decimals as i32);
            I256::from_dec_str(amount.to_string().split('.').next().unwrap())?
        } else {
            // Convert amount to token B (ETH) decimals
            let amount = amount * 10_f64.powi(pool.token_b_decimals as i32);
            I256::from_dec_str(amount.to_string().split('.').next().unwrap())?
        };

        let (swap_amount_in, swap_amount_out) = pool.simulate_swap(token_in, amount_in, None)?;

        let amount_in = U256::try_from(swap_amount_in.abs())?;
        let amount_out = U256::try_from(swap_amount_out.abs())?;

        Ok(self.create_order(amount_in, amount_out))
    }

    fn create_order(&self, amount_in: U256, amount_out: U256) -> TopOfBlockOrder {
        TopOfBlockOrder {
            quantity_in: u128::try_from(amount_in).unwrap(),
            quantity_out: u128::try_from(amount_out).unwrap(),
            ..TopOfBlockOrder::default()
        }
    }
}

#[derive(Debug, Error)]
pub enum UserOrderError {
    #[error(transparent)]
    PoolError(#[from] PoolError),
    #[error(transparent)]
    BigIntConversionError(#[from] BigIntConversionError),
    #[error(transparent)]
    ParseSignedError(#[from] ParseSignedError),
    #[error(transparent)]
    SwapSimulationError(#[from] SwapSimulationError)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_custom_range_distribution() {
        let distribution = PriceDistribution::new(0.8, 0.1, 100.0).unwrap();
        // Generate multiple orders to test the distribution
        let num_samples = 1000;
        let mut samples = Vec::with_capacity(num_samples);
        let mut rng = rand::thread_rng();

        for _ in 0..num_samples {
            let amount = distribution.sample(&mut rng);
            samples.push(amount);
        }

        // Check that all samples are within the specified range
        for sample in &samples {
            assert!(*sample >= 0.1 && *sample <= 100.0, "Sample {} is out of range", sample);
        }

        // Calculate mean and ensure it's roughly in the middle of the range
        let mean = samples.iter().sum::<f64>() / samples.len() as f64;
        assert!(mean > 10.0 && mean < 20.0, "Mean {} is not correctly skewed", mean);

        // Check that we have a good distribution of values across the range
        let mut binned_samples = [0; 10];
        for sample in samples {
            let bin = ((sample - 0.1) / (100.0 - 0.1) * 10.0).floor() as usize;
            let bin = bin.min(9); // Ensure we don't go out of bounds
            binned_samples[bin] += 1;
        }
        // Print the bins
        println!("Distribution of samples across bins:");
        for (i, count) in binned_samples.iter().enumerate() {
            let lower_bound = 0.1 + (i as f64 * 10.0);
            let upper_bound = 0.1 + ((i + 1) as f64 * 10.0);
            println!("Bin {} ({:.2} - {:.2}): {} samples", i, lower_bound, upper_bound, count);
        }

        // Ensure each bin has a reasonable number of samples
        for (i, count) in binned_samples.iter().enumerate() {
            if i < 6 {
                assert!(*count > 0, "Bin {} has no samples", i);
            }
            assert!(*count < (num_samples * 3 / 4), "Bin {} has too many samples", i);
        }
    }

    #[test]
    fn test_order_count_distribution() {
        let config = OrderGeneratorConfig::default();
        let order_count_distribution = Normal::new(0.0, config.std_dev_orders).unwrap();
        let num_samples = 10000;
        let mut rng = rand::thread_rng();

        let samples: Vec<u32> = (0..num_samples)
            .map(|_| {
                let raw_sample = order_count_distribution.sample(&mut rng);
                let shifted_sample = raw_sample.abs();
                let fat_tailed_sample = shifted_sample.powf(2.0);
                (fat_tailed_sample.round() as u32).min(20)
            })
            .collect();

        let mean = samples.iter().sum::<u32>() as f64 / num_samples as f64;
        let variance = samples
            .iter()
            .map(|&x| (x as f64 - mean).powi(2))
            .sum::<f64>()
            / num_samples as f64;
        let std_dev = variance.sqrt();

        println!("Order count distribution:");
        println!("Mean: {:.2}", mean);
        println!("Standard Deviation: {:.2}", std_dev);

        // Check if the distribution is roughly as expected
        assert!(mean > 0.0 && mean < 5.0, "Mean should be between 0 and 5");
        assert!(std_dev > 0.0 && std_dev < 5.0, "Standard deviation should be between 0 and 5");

        // Check for fat tails
        let max_count = *samples.iter().max().unwrap();
        assert!(max_count > 10, "Should have some samples above 10 for fat tails");
        assert!(max_count <= 20, "Should not exceed the maximum of 20 orders");
    }
}
