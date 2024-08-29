use std::cmp::Ordering;
use std::sync::Arc;

use crate::cfmm::uniswap::pool_manager::PoolManagerError;
use alloy::{
    network::Network,
    primitives::{Address, I256, U256},
    providers::Provider,
    sol,
    sol_types::{SolEvent, SolType},
    transports::Transport
};
use amms::amm::{
    consts::U256_1,
    uniswap_v3::{IUniswapV3Pool, Info, UniswapV3Pool},
    AutomatedMarketMaker
};
use amms::errors::{AMMError, EventLogError};
use num_bigfloat::BigFloat;
use rand_distr::num_traits::One;
use reth_primitives::Log;
use thiserror::Error;
use uniswap_v3_math::{
    error::UniswapV3MathError,
    tick_math::{MAX_SQRT_RATIO, MAX_TICK, MIN_SQRT_RATIO, MIN_TICK}
};

sol! {
    #[allow(missing_docs)]
    #[sol(rpc)]
    IGetUniswapV3TickDataBatchRequest,
    "src/cfmm/uniswap/GetUniswapV3TickDataBatchRequestABI.json"
}

sol! {
    struct TickData {
        bool initialized;
        int24 tick;
        uint128 liquidityGross;
        int128 liquidityNet;
    }

    struct TicksWithBlock {
        TickData[] ticks;
        uint256 blockNumber;
    }
}

#[derive(Debug, Clone)]
pub struct UniswapV3TickData {
    pub initialized:     bool,
    pub tick:            i32,
    pub liquidity_gross: u128,
    pub liquidity_net:   i128
}

#[derive(Default)]
struct SwapResult {
    amount0:         I256,
    amount1:         I256,
    sqrt_price_x_96: U256,
    liquidity:       u128,
    tick:            i32
}

// at around 190 is when "max code size exceeded" comes up
const MAX_TICKS_PER_REQUEST: u16 = 150;

#[derive(Debug, Clone)]
pub struct EnhancedUniswapV3Pool {
    inner:                  UniswapV3Pool,
    sync_swap_with_sim:     bool,
    initial_ticks_per_side: u16
}

impl EnhancedUniswapV3Pool {
    pub fn new(address: Address, initial_ticks_per_side: u16) -> Self {
        Self {
            inner: UniswapV3Pool { address, ..Default::default() },
            initial_ticks_per_side,
            sync_swap_with_sim: false
        }
    }

    pub fn set_sim_swap_sync(&mut self, sync_swap_with_sim: bool) {
        self.sync_swap_with_sim = sync_swap_with_sim;
    }

    pub async fn get_uniswap_v3_tick_data_batch_request<P, T, N>(
        &self,
        tick_start: i32,
        zero_for_one: bool,
        num_ticks: u16,
        block_number: Option<u64>,
        provider: Arc<P>
    ) -> Result<(Vec<UniswapV3TickData>, U256), AMMError>
    where
        P: Provider<T, N>,
        T: Transport + Clone,
        N: Network
    {
        let deployer = IGetUniswapV3TickDataBatchRequest::deploy_builder(
            provider.clone(),
            self.address,
            zero_for_one,
            tick_start,
            num_ticks,
            self.tick_spacing
        );

        let data = match block_number {
            Some(number) => deployer.block(number.into()).call_raw().await?,
            None => deployer.call_raw().await?
        };

        let result = TicksWithBlock::abi_decode(&data, true)?;

        let tick_data: Vec<UniswapV3TickData> = result
            .ticks
            .iter()
            .map(|tick| UniswapV3TickData {
                initialized:     tick.initialized,
                tick:            tick.tick,
                liquidity_gross: tick.liquidityGross,
                liquidity_net:   tick.liquidityNet
            })
            .collect();

        Ok((tick_data, result.blockNumber))
    }

    pub fn exchange_price(&self, quoted_asset: Option<Address>) -> Result<f64, PoolError> {
        let tick = uniswap_v3_math::tick_math::get_tick_at_sqrt_ratio(self.sqrt_price)?;
        let shift = self.token_a_decimals as i8 - self.token_b_decimals as i8;

        let price = match shift.cmp(&0) {
            Ordering::Less => 1.0001_f64.powi(tick) / 10_f64.powi(-shift as i32),
            Ordering::Greater => 1.0001_f64.powi(tick) * 10_f64.powi(shift as i32),
            Ordering::Equal => 1.0001_f64.powi(tick),
        };

        if quoted_asset.is_none() || quoted_asset.unwrap() == self.token_b {
            Ok(1.0 / price)
        } else {
            Ok(price)
        }
    }

    pub fn exchange_quantity(&self, asset: Option<Address>) -> Result<u128, PoolError> {
        let (reserve_a, reserve_b) = self.calculate_virtual_reserves()?;
        match asset {
            Some(asset) if asset == self.token_a => Ok(reserve_a),
            Some(asset) if asset == self.token_b => Ok(reserve_b),
            _ => Ok(reserve_a),
        }
    }

    pub async fn initialize<T, N, P>(
        &mut self,
        block_number: Option<u64>,
        provider: Arc<P>
    ) -> Result<(), PoolError>
    where
        T: Transport + Clone,
        N: Network,
        P: Provider<T, N>
    {
        self.populate_data(block_number, provider).await.map_err(PoolError::from)
    }

    pub fn is_initialized(&self) -> bool {
        self.token_a != Address::default() && self.token_b != Address::default()
    }

    pub async fn sync_ticks<T, N, P>(
        &mut self,
        block_number: Option<u64>,
        provider: Arc<P>
    ) -> Result<(), PoolError>
    where
        T: Transport + Clone,
        N: Network,
        P: Provider<T, N>
    {
        if !self.is_initialized() {
            return Err(PoolError::PoolDataError);
        }

        self.ticks.clear();
        self.tick_bitmap.clear();

        let total_ticks_to_fetch = self.initial_ticks_per_side * 2;
        let mut remaining_ticks = total_ticks_to_fetch;
        //  +1 because the retrieve is gt start_tick, i.e. start one step back to
        // include the tick
        let mut start_tick = (self.tick / self.tick_spacing) * self.tick_spacing
            - self.tick_spacing * (self.initial_ticks_per_side + 1) as i32;

        // Fetch ticks from left to right
        let mut fetched_ticks = Vec::new();
        while remaining_ticks > 0 {
            let ticks_to_fetch = remaining_ticks.min(MAX_TICKS_PER_REQUEST);
            let (mut batch_ticks, _) = self
                .get_uniswap_v3_tick_data_batch_request(
                    start_tick,
                    false,
                    ticks_to_fetch,
                    block_number,
                    provider.clone()
                )
                .await?;
            batch_ticks.sort_by_key(|s| s.tick);
            fetched_ticks.append(&mut batch_ticks);
            remaining_ticks -= ticks_to_fetch;
            if let Some(last_tick) = fetched_ticks.last() {
                start_tick = last_tick.tick;
            } else {
                break;
            }
        }

        fetched_ticks
            .into_iter()
            .filter(|tick| tick.initialized)
            .for_each(|tick| {
                self.ticks.insert(
                    tick.tick,
                    Info {
                        initialized:     tick.initialized,
                        liquidity_gross: tick.liquidity_gross,
                        liquidity_net:   tick.liquidity_net
                    }
                );
                self.inner.flip_tick(tick.tick, self.inner.tick_spacing);
            });

        Ok(())
    }

    /// Obvious doc: Sims the swap to get the state changes after applying it
    ///
    /// (maybe) Not so obvious doc:
    ///     * Testing:    If the goal is to test the implementation, passing
    ///       amount0 *and* limit price, will mess with your testing
    ///       reliability, since you'd be clamping the potential change in
    ///       amount1, i.e. you probably won't be testing much.
    ///     * Sync logs:  Swap sync logs don't have the zeroForOne field, which
    ///       coupled with amountSpecified produces 4 possible combinations of
    ///       parameter. Therefore, if you are syncing from swap log, you need
    ///       to try out all of the combinations below, to know exactly with
    ///       which set of zeroForOne x amountSpecified parameters the sim
    ///       method was called
    ///
    ///       let combinations = [
    ///           (pool.token_a, swap_event.amount0),
    ///           (pool.token_b, swap_event.amount0),
    ///           (pool.token_a, swap_event.amount1),
    ///           (pool.token_b, swap_event.amount1),
    ///       ];
    fn _simulate_swap(
        &self,
        token_in: Address,
        amount_specified: I256,
        sqrt_price_limit_x96: Option<U256>
    ) -> Result<SwapResult, PoolError> {
        if amount_specified.is_zero() {
            return Err(PoolError::ZeroAmountSpecified);
        }

        let zero_for_one = token_in == self.token_a;
        let exact_input = amount_specified.is_positive();

        let sqrt_price_limit_x96 = sqrt_price_limit_x96.unwrap_or(if zero_for_one {
            MIN_SQRT_RATIO + U256_1
        } else {
            MAX_SQRT_RATIO - U256_1
        });

        if (zero_for_one
            && (sqrt_price_limit_x96 >= self.sqrt_price || sqrt_price_limit_x96 <= MIN_SQRT_RATIO))
            || (!zero_for_one
                && (sqrt_price_limit_x96 <= self.sqrt_price
                    || sqrt_price_limit_x96 >= MAX_SQRT_RATIO))
        {
            return Err(PoolError::InvalidSqrtPriceLimit);
        }

        let mut amount_specified_remaining = amount_specified;
        let mut amount_calculated = I256::ZERO;
        let mut sqrt_price_x_96 = self.sqrt_price;
        let mut tick = self.tick;
        let mut liquidity = self.liquidity;

        tracing::trace!(
            token_in = ?token_in,
            amount_specified = ?amount_specified,
            zero_for_one = zero_for_one,
            exact_input = exact_input,
            sqrt_price_limit_x96 = ?sqrt_price_limit_x96,
            initial_state = ?(
                &amount_specified_remaining,
                &amount_calculated,
                &sqrt_price_x_96,
                &tick,
                &liquidity
            ),
            "starting swap"
        );

        while amount_specified_remaining != I256::ZERO && sqrt_price_x_96 != sqrt_price_limit_x96 {
            let sqrt_price_start_x_96 = sqrt_price_x_96;
            let (tick_next, initialized) =
                uniswap_v3_math::tick_bitmap::next_initialized_tick_within_one_word(
                    &self.tick_bitmap,
                    tick,
                    self.tick_spacing,
                    zero_for_one
                )?;

            let tick_next = tick_next.clamp(MIN_TICK, MAX_TICK);
            let sqrt_price_next_x96 =
                uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick(tick_next)?;

            let target_sqrt_ratio = if (zero_for_one && sqrt_price_next_x96 < sqrt_price_limit_x96)
                || (!zero_for_one && sqrt_price_next_x96 > sqrt_price_limit_x96)
            {
                sqrt_price_limit_x96
            } else {
                sqrt_price_next_x96
            };

            let (new_sqrt_price_x_96, amount_in, amount_out, fee_amount) =
                uniswap_v3_math::swap_math::compute_swap_step(
                    sqrt_price_x_96,
                    target_sqrt_ratio,
                    liquidity,
                    amount_specified_remaining,
                    self.fee
                )?;

            sqrt_price_x_96 = new_sqrt_price_x_96;

            if exact_input {
                amount_specified_remaining -= I256::from_raw(amount_in + fee_amount);
                amount_calculated -= I256::from_raw(amount_out);
            } else {
                amount_specified_remaining += I256::from_raw(amount_out);
                amount_calculated += I256::from_raw(amount_in + fee_amount);
            }

            if sqrt_price_x_96 == sqrt_price_next_x96 {
                if initialized {
                    let liquidity_net =
                        self.ticks
                            .get(&tick_next)
                            .map(|info| {
                                if zero_for_one {
                                    -info.liquidity_net
                                } else {
                                    info.liquidity_net
                                }
                            })
                            .unwrap_or_default();

                    liquidity = if liquidity_net < 0 {
                        liquidity
                            .checked_sub((-liquidity_net) as u128)
                            .ok_or(PoolError::LiquidityUnderflow)?
                    } else {
                        liquidity + (liquidity_net as u128)
                    };
                }

                tick = if zero_for_one { tick_next - 1 } else { tick_next };
            } else if sqrt_price_x_96 != sqrt_price_start_x_96 {
                tick = uniswap_v3_math::tick_math::get_tick_at_sqrt_ratio(sqrt_price_x_96)?;
            }

            tracing::trace!(
                sqrt_price_x_96 = ?sqrt_price_x_96,
                amount_in = ?amount_in,
                amount_out = ?amount_out,
                fee_amount = ?fee_amount,
                tick_next = ?tick_next,
                state = ?(
                    &amount_specified_remaining,
                    &amount_calculated,
                    &sqrt_price_x_96,
                    &tick,
                    &liquidity
                ),
                "step completed"
            );
        }

        let (amount0, amount1) = if zero_for_one == exact_input {
            (amount_specified - amount_specified_remaining, amount_calculated)
        } else {
            (amount_calculated, amount_specified - amount_specified_remaining)
        };

        tracing::debug!(?amount0, ?amount1);

        Ok(SwapResult { amount0, amount1, liquidity, sqrt_price_x_96, tick })
    }

    pub fn simulate_swap(
        &self,
        token_in: Address,
        amount_specified: I256,
        sqrt_price_limit_x96: Option<U256>
    ) -> Result<(I256, I256), PoolError> {
        let swap_result = self._simulate_swap(token_in, amount_specified, sqrt_price_limit_x96)?;
        Ok((swap_result.amount0, swap_result.amount1))
    }

    pub fn simulate_swap_mut(
        &mut self,
        token_in: Address,
        amount_specified: I256,
        sqrt_price_limit_x96: Option<U256>
    ) -> Result<(I256, I256), PoolError> {
        let swap_result = self._simulate_swap(token_in, amount_specified, sqrt_price_limit_x96)?;

        self.liquidity = swap_result.liquidity;
        self.sqrt_price = swap_result.sqrt_price_x_96;
        self.tick = swap_result.tick;

        Ok((swap_result.amount0, swap_result.amount1))
    }

    pub fn sync_from_swap_log(&mut self, log: Log) -> Result<(), PoolError> {
        if self.sync_swap_with_sim {
            self.sync_swap_with_sim(log)
        } else {
            self._sync_from_swap_log(log).map_err(Into::into)
        }
    }

    fn sync_swap_with_sim(&mut self, log: Log) -> Result<(), PoolError> {
        let swap_event = IUniswapV3Pool::Swap::decode_log(&log, true)?;

        tracing::trace!(pool_tick = ?self.tick, pool_price = ?self.sqrt_price, pool_liquidity = ?self.liquidity, pool_address = ?self.address, "pool before");
        tracing::debug!(swap_tick=swap_event.tick, swap_price=?swap_event.sqrtPriceX96, swap_liquidity=?swap_event.liquidity, swap_amount0=?swap_event.amount0, swap_amount1=?swap_event.amount1, "swap event");

        let combinations = [
            (self.token_b, swap_event.amount1),
            (self.token_a, swap_event.amount0),
            (self.token_a, swap_event.amount1),
            (self.token_b, swap_event.amount0)
        ];

        let mut simulation_failed = true;
        for (token_in, amount_in) in combinations.iter() {
            let sqrt_price_limit_x96 = Some(swap_event.sqrtPriceX96);
            if let Ok((amount0, amount1)) =
                self.simulate_swap(*token_in, *amount_in, sqrt_price_limit_x96)
            {
                if swap_event.amount0 == amount0 && swap_event.amount1 == amount1 {
                    // will not fail
                    let (..) =
                        self.simulate_swap_mut(*token_in, *amount_in, sqrt_price_limit_x96)?;
                    simulation_failed = false;
                    break;
                }
            }
        }

        if simulation_failed {
            tracing::error!(
                pool_address = ?self.address,
                pool_price = ?self.sqrt_price,
                pool_liquidity = ?self.liquidity,
                pool_tick = ?self.tick,
                swap_price = ?swap_event.sqrtPriceX96,
                swap_tick = swap_event.tick,
                swap_liquidity = ?swap_event.liquidity,
                swap_amount0 = ?swap_event.amount0,
                swap_amount1 = ?swap_event.amount1,
                "Swap simulation failed"
            );
            return Err(PoolError::SwapSimulationFailed);
        } else {
            tracing::trace!(pool_tick = ?self.tick, pool_price = ?self.sqrt_price, pool_liquidity = ?self.liquidity, pool_address = ?self.address, "pool after");
        }

        Ok(())
    }

    pub fn sync_from_log(&mut self, log: Log) -> Result<(), PoolError> {
        let event_signature = log.topics()[0];

        if event_signature == IUniswapV3Pool::Burn::SIGNATURE_HASH {
            self.sync_from_burn_log(log)?;
        } else if event_signature == IUniswapV3Pool::Mint::SIGNATURE_HASH {
            self.sync_from_mint_log(log)?;
        } else if event_signature == IUniswapV3Pool::Swap::SIGNATURE_HASH {
            self._sync_from_swap_log(log)?;
        } else {
            Err(PoolError::InvalidEventSignature)?
        }

        Ok(())
    }

    pub fn sync_from_burn_log(&mut self, log: Log) -> Result<(), alloy::dyn_abi::Error> {
        let burn_event = IUniswapV3Pool::Burn::decode_log(&log, true)?;

        self.modify_position(
            burn_event.tickLower,
            burn_event.tickUpper,
            -(burn_event.amount as i128)
        );

        tracing::debug!(?burn_event, address = ?self.address, sqrt_price = ?self.sqrt_price, liquidity = ?self.liquidity, tick = ?self.tick, "burn event");

        Ok(())
    }

    pub fn sync_from_mint_log(&mut self, log: Log) -> Result<(), alloy::dyn_abi::Error> {
        let mint_event = IUniswapV3Pool::Mint::decode_log(&log, true)?;

        self.modify_position(mint_event.tickLower, mint_event.tickUpper, mint_event.amount as i128);

        tracing::debug!(?mint_event, address = ?self.address, sqrt_price = ?self.sqrt_price, liquidity = ?self.liquidity, tick = ?self.tick, "mint event");

        Ok(())
    }

    pub fn _sync_from_swap_log(&mut self, log: Log) -> Result<(), alloy::sol_types::Error> {
        let swap_event = IUniswapV3Pool::Swap::decode_log(&log, true)?;

        self.sqrt_price = swap_event.sqrtPriceX96;
        self.liquidity = swap_event.liquidity;
        self.tick = swap_event.tick;

        tracing::debug!(?swap_event, address = ?self.address, sqrt_price = ?self.sqrt_price, liquidity = ?self.liquidity, tick = ?self.tick, "swap event");

        Ok(())
    }
}

impl std::ops::Deref for EnhancedUniswapV3Pool {
    type Target = UniswapV3Pool;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl std::ops::DerefMut for EnhancedUniswapV3Pool {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

#[derive(Error, Debug)]
pub enum SwapSimulationError {
    #[error("Could not get next tick")]
    InvalidTick,
    #[error(transparent)]
    UniswapV3MathError(#[from] UniswapV3MathError),
    #[error("Liquidity underflow")]
    LiquidityUnderflow,
    #[error("Invalid sqrt price limit")]
    InvalidSqrtPriceLimit,
    #[error("Amount specified must be non-zero")]
    ZeroAmountSpecified
}

#[derive(Error, Debug)]
pub enum PoolError {
    #[error("Could not get next tick")]
    InvalidTick,
    #[error(transparent)]
    PoolError(#[from] SwapSimulationError),
    #[error(transparent)]
    UniswapV3MathError(#[from] UniswapV3MathError),
    #[error("Liquidity underflow")]
    LiquidityUnderflow,
    #[error("Invalid sqrt price limit")]
    InvalidSqrtPriceLimit,
    #[error("Amount specified must be non-zero")]
    ZeroAmountSpecified,
    #[error("Invalid asset")]
    InvalidAsset,
    #[error("U64 conversion error")]
    U64ConversionError,
    #[error("Pool data error")]
    PoolDataError,
    #[error("Swap simulation failed")]
    SwapSimulationFailed,
    #[error("Invalid event signature")]
    InvalidEventSignature,
    #[error(transparent)]
    EthABIError(#[from] alloy::sol_types::Error),
    #[error(transparent)]
    ABIError(#[from] alloy::dyn_abi::Error),
    #[error(transparent)]
    AMMError(#[from] amms::errors::AMMError),
    #[error(transparent)]
    ArithmeticError(#[from] amms::errors::ArithmeticError),
}

#[cfg(test)]
mod test {
    use super::*;
    use alloy::{
        hex,
        network::Ethereum,
        primitives::{address, Bytes, Log as AlloyLog, LogData, B256, U256},
        providers::{ProviderBuilder, RootProvider},
        rpc::client::ClientBuilder,
        transports::{
            http::{Client, Http},
            layers::{RetryBackoffLayer, RetryBackoffService}
        }
    };
    use std::{str::FromStr, sync::Arc};

    async fn setup_provider() -> Arc<RootProvider<RetryBackoffService<Http<Client>>, Ethereum>> {
        let rpc_endpoint =
            std::env::var("ETHEREUM_RPC_ENDPOINT").expect("ETHEREUM_RPC_ENDPOINT must be set");
        let url = rpc_endpoint.parse().unwrap();
        let client = ClientBuilder::default()
            .layer(RetryBackoffLayer::new(4, 1000, 100))
            .http(url);
        let provider: RootProvider<RetryBackoffService<Http<Client>>, Ethereum> =
            ProviderBuilder::default().on_client(client);
        Arc::new(provider)
    }

    async fn setup_pool(
        provider: Arc<RootProvider<RetryBackoffService<Http<Client>>, Ethereum>>,
        block_number: u64,
        ticks_per_side: u16
    ) -> EnhancedUniswapV3Pool {
        let address = address!("88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640");
        let mut pool = EnhancedUniswapV3Pool::new(address, ticks_per_side);
        pool.initialize(Some(block_number), provider.clone())
            .await
            .unwrap();
        pool
    }

    #[tokio::test]
    async fn test_load_pool() {
        let block_number = 20498069;
        let ticks_per_side = 10;
        let provider = setup_provider().await;
        let pool = setup_pool(provider.clone(), block_number, ticks_per_side).await;

        assert_eq!(pool.address, address!("88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640"));
        assert_eq!(pool.token_a, address!("A0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"));
        assert_eq!(pool.token_a_decimals, 6);
        assert_eq!(pool.token_b, address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"));
        assert_eq!(pool.token_b_decimals, 18);
        assert!(pool.liquidity > 0);
        assert!(pool.sqrt_price > U256::ZERO);
        assert_eq!(pool.fee, 500);
        assert_eq!(pool.tick, 197547);
        assert_eq!(pool.tick_spacing, 10);
        assert!(pool.tick_bitmap.is_empty());
        assert!(pool.ticks.is_empty());
    }

    #[tokio::test]
    async fn test_swap_in_pool() {
        let swap_block = 20480828;
        let previous_block = swap_block - 1;
        let ticks_per_side = 10;
        let provider = setup_provider().await;
        let mut pool = setup_pool(provider.clone(), previous_block, ticks_per_side).await;
        pool.sync_ticks(Some(previous_block), provider.clone())
            .await
            .expect("failed to sync ticks");

        // Check pool state before the swap
        assert_eq!(pool.liquidity, 2694411943070307563, "Liquidity mismatch before swap");
        assert_eq!(
            pool.sqrt_price,
            U256::from_str("1598677116547625698577517567213640").unwrap(),
            "Sqrt price mismatch before swap"
        );
        assert_eq!(pool.tick, 198257, "Tick mismatch before swap");

        // Perform the swap
        let token_in = pool.token_a;
        let amount_in = I256::from_str("66741928781").unwrap(); // 66741.928781 * 10^6
        let (amount_0, amount_1) = pool
            .simulate_swap_mut(token_in, amount_in, None)
            .expect("Swap simulation failed");

        // Check the results
        assert_eq!(amount_0, I256::from_str("66741928781").unwrap(), "Incorrect amount in");
        assert_eq!(
            amount_1,
            I256::from_str("-27147321967958680641").unwrap(),
            "Incorrect amount out"
        );
        assert_eq!(
            pool.sqrt_price,
            U256::from_str("1597878859828850322653524929880487").unwrap(),
            "Incorrect sqrtPriceX96"
        );
        assert_eq!(pool.liquidity, 2694411943070307563, "Incorrect liquidity");
        assert_eq!(pool.tick, 198247, "Incorrect tick");

        // Compare with the state after the swap
        let mut after_swap_pool = setup_pool(provider.clone(), swap_block, ticks_per_side).await;
        after_swap_pool
            .sync_ticks(Some(swap_block), provider.clone())
            .await
            .expect("failed to sync ticks");

        assert_eq!(pool.tick_spacing, after_swap_pool.tick_spacing, "Tick spacing mismatch");
        assert_eq!(pool.liquidity, after_swap_pool.liquidity, "Liquidity mismatch");
        assert_eq!(pool.sqrt_price, after_swap_pool.sqrt_price, "Sqrt price mismatch");
        assert_eq!(pool.tick, after_swap_pool.tick, "Tick mismatch");
        assert_eq!(pool.tick_bitmap, after_swap_pool.tick_bitmap, "Tick bitmap mismatch");
    }

    #[tokio::test]
    async fn test_burn_pool_outside_range() {
        let burn_block = 20505115;
        let previous_block = burn_block - 1;
        // needs a fairly wide range
        let ticks_per_side = 250;
        let provider = setup_provider().await;
        let mut pool = setup_pool(provider.clone(), previous_block, ticks_per_side).await;
        pool.sync_ticks(Some(previous_block), provider.clone())
            .await
            .expect("failed to sync ticks");
        // owner: 0xC36442b4a4522E871399CD717aBDD847Ab11FE88, tickLower: 195090,
        // tickUpper: 195540, amount: 136670924187209102, amount0: 0, amount1:
        // 53560594103808093362
        pool.sync_from_burn_log(AlloyLog {
            address: address!("88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640"),
            data: LogData::new(
                vec![
                    B256::from_slice(&hex::decode("0c396cd989a39f4459b5fa1aed6a9a8dcdbc45908acfd67e028cd568da98982c").unwrap()),
                    B256::from_slice(&hex::decode("000000000000000000000000c36442b4a4522e871399cd717abdd847ab11fe88").unwrap()),
                    B256::from_slice(&hex::decode("000000000000000000000000000000000000000000000000000000000002fa12").unwrap()),
                    B256::from_slice(&hex::decode("000000000000000000000000000000000000000000000000000000000002fbd4").unwrap()),
                ],
                Bytes::copy_from_slice(&hex::decode("00000000000000000000000000000000000000000000000001e58d7b3f4d8d8e0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000002e74d748fac5cecb2").unwrap()),
            ).unwrap(),
        }).unwrap();

        let mut after_burn_pool = setup_pool(provider.clone(), burn_block, ticks_per_side).await;
        after_burn_pool
            .sync_ticks(Some(burn_block), provider.clone())
            .await
            .expect("failed to sync ticks");

        // Compare fields of after_burn_pool and pool
        assert_eq!(pool.address, after_burn_pool.address, "Address mismatch");
        assert_eq!(pool.token_a, after_burn_pool.token_a, "Token A mismatch");
        assert_eq!(pool.token_b, after_burn_pool.token_b, "Token B mismatch");
        assert_eq!(
            pool.token_a_decimals, after_burn_pool.token_a_decimals,
            "Token A decimals mismatch"
        );
        assert_eq!(
            pool.token_b_decimals, after_burn_pool.token_b_decimals,
            "Token B decimals mismatch"
        );
        assert_eq!(pool.fee, after_burn_pool.fee, "Fee mismatch");
        assert_eq!(pool.tick_spacing, after_burn_pool.tick_spacing, "Tick spacing mismatch");
        assert_eq!(pool.liquidity, after_burn_pool.liquidity, "Liquidity mismatch");
        assert_eq!(pool.sqrt_price, after_burn_pool.sqrt_price, "Sqrt price mismatch");
        assert_eq!(pool.tick, after_burn_pool.tick, "Tick mismatch");
        assert_eq!(pool.tick_bitmap, after_burn_pool.tick_bitmap, "Tick bitmap mismatch");
        assert_eq!(pool.ticks.len(), after_burn_pool.ticks.len(), "Number of ticks mismatch");
        for (tick, info) in pool.ticks.iter() {
            let after_burn_info = after_burn_pool
                .ticks
                .get(tick)
                .expect("Tick not found in after_burn_pool");
            assert_eq!(
                info.liquidity_gross, after_burn_info.liquidity_gross,
                "Liquidity gross mismatch for tick {}",
                tick
            );
            assert_eq!(
                info.liquidity_net, after_burn_info.liquidity_net,
                "Liquidity net mismatch for tick {}",
                tick
            );
            assert_eq!(
                info.initialized, after_burn_info.initialized,
                "Initialized flag mismatch for tick {}",
                tick
            );
        }
    }

    #[tokio::test]
    async fn test_load_ticks() {
        let block_number = 20498069;
        let ticks_per_side = 10;
        let provider = setup_provider().await;
        let mut pool = setup_pool(provider.clone(), block_number, ticks_per_side).await;
        pool.sync_ticks(Some(block_number), provider.clone())
            .await
            .expect("failed to sync ticks");

        assert_eq!(pool.ticks.len(), 20, "No ticks were loaded");
        let expected_ticks = vec![
            (197440, 111865134864137602, 102733576284820334),
            (197450, 2771971676516941, -2771834237563469),
            (197460, 3859798908378, 3722359954906),
            (197470, 1193748914015, -1193748914015),
            (197480, 44433936013305, -44433936013305),
            (197500, 309999197921697743, 273427767344046023),
            (197520, 192996722135755, 192996722135755),
            (197530, 539241987876, 539241987876),
            (197540, 13222195433615963, 11816097785443223),
            (197560, 27595709583757, 27595709583757),
            (197570, 6112184035950571, -5149755217930223),
            (197580, 190509459765420, -77595848930828),
            (197590, 4325345455099573, 4139136533538185),
            (197600, 92637809125804, -92637809125804),
            (197610, 2047252361319, -2047252361319),
            (197620, 14063986970207, 5353547592293),
            (197630, 1349269385245271, 1345601874435831),
            (197640, 6516724029717570, 6516724029717570),
            (197650, 7142580627476203506, 7142569105915081148),
            (197670, 560640048101896452, 516286964688874506),
        ];

        for (tick, expected_gross, expected_net) in expected_ticks {
            let tick_data = pool.ticks.get(&tick);
            assert!(tick_data.is_some(), "tick {} should be initialized", tick);

            let info = tick_data.unwrap();
            assert_eq!(
                info.liquidity_gross, expected_gross,
                "Mismatch in liquidity_gross for tick {}",
                tick
            );
            assert_eq!(
                info.liquidity_net, expected_net,
                "Mismatch in liquidity_net for tick {}",
                tick
            );
            assert!(info.initialized, "Tick {} should be initialized", tick);
        }
    }

    #[tokio::test]
    async fn test_multiple_swaps() {
        let block_number = 20522215;
        let ticks_per_side = 200;
        let provider = setup_provider().await;
        let mut pool = setup_pool(provider.clone(), block_number, ticks_per_side).await;
        pool.sync_ticks(Some(block_number), provider.clone())
            .await
            .expect("failed to sync ticks");

        assert_eq!(
            pool.sqrt_price,
            U256::from_str_radix("1522541228652157746214186795710203", 10).unwrap()
        );
        assert_eq!(pool.liquidity, 14623537689052122812u128);
        assert_eq!(pool.tick, 197281);

        // First swap
        let token_in = pool.token_b;
        let amount_in = I256::from_dec_str("300532960990132029").unwrap();
        let (amount_0, amount_1) = pool
            .simulate_swap_mut(token_in, amount_in, None)
            .expect("Fist swap simulation failed");
        assert_eq!(amount_0, I256::from_dec_str("-813383744").unwrap());
        assert_eq!(amount_1, I256::from_dec_str("300532960990132029").unwrap());
        assert_eq!(
            pool.sqrt_price,
            U256::from_str_radix("1522542856081131714601312592562953", 10).unwrap()
        );
        assert_eq!(pool.liquidity, 14623537689052122812u128);
        assert_eq!(pool.tick, 197281);

        // Second swap causing issues
        let token_in = pool.token_b;
        let amount_in = I256::from_dec_str("-100000000").unwrap();
        let (amount_0, amount_1) = pool
            .simulate_swap_mut(token_in, amount_in, None)
            .expect("Second swap simulation failed");
        assert_eq!(amount_0, I256::from_dec_str("-100000000").unwrap());
        assert_eq!(amount_1, I256::from_dec_str("36948528148148111").unwrap());
        assert_eq!(
            pool.sqrt_price,
            U256::from_str_radix("1522543056162696996744021728687215", 10).unwrap()
        );
        assert_eq!(pool.liquidity, 14623537689052122812u128);
        assert_eq!(pool.tick, 197281);

        // Third swap
        let token_in = pool.token_b;
        let amount_in = I256::from_dec_str("-111610000").unwrap();
        let (amount_0, amount_1) = pool
            .simulate_swap_mut(token_in, amount_in, None)
            .expect("Third swap simulation failed");
        assert_eq!(amount_0, I256::from_dec_str("-111610000").unwrap());
        assert_eq!(amount_1, I256::from_dec_str("41238263733788147").unwrap());
        assert_eq!(
            pool.sqrt_price,
            U256::from_str_radix("1522543279473794107054723771988160", 10).unwrap()
        );
        assert_eq!(pool.liquidity, 14623537689052122812u128);
        assert_eq!(pool.tick, 197281);
    }
}
