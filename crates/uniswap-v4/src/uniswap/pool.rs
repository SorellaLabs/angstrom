use std::{cmp::Ordering, collections::HashMap, fmt::Debug, ops::Rem, sync::Arc};

use alloy::{
    hex,
    primitives::{Address, B256, BlockNumber, I256, U256, aliases::I24},
    providers::Provider,
    transports::Transport
};
use alloy_primitives::Log;
use angstrom_types::{
    matching::uniswap::{LiqRange, PoolSnapshot},
    primitive::PoolId
};
use itertools::Itertools;
use malachite::num::conversion::traits::SaturatingInto;
use thiserror::Error;
use uniswap_v3_math::{
    error::UniswapV3MathError,
    tick_math::{MAX_SQRT_RATIO, MAX_TICK, MIN_SQRT_RATIO, MIN_TICK}
};

use super::{pool_data_loader::PoolData, pool_manager::TickRangeToLoad};
use crate::uniswap::{
    ConversionError, i32_to_i24,
    pool_data_loader::{DataLoader, ModifyPositionEvent, PoolDataLoader, TickData}
};

#[derive(Default)]
struct SwapResult {
    amount0:         I256,
    amount1:         I256,
    sqrt_price_x_96: U256,
    liquidity:       u128,
    tick:            i32
}

#[derive(Debug, Clone, Default)]
pub struct TickInfo {
    pub liquidity_gross: u128,
    pub liquidity_net:   i128,
    pub initialized:     bool
}

// at around 190 is when "max code size exceeded" comes up
// const MAX_TICKS_PER_REQUEST: u16 = 150;

pub const U256_1: U256 = U256::from_limbs([1, 0, 0, 0]);

const TICKS_PER_BATCH: usize = 25;

#[derive(Debug, Clone, Default)]
pub struct EnhancedUniswapPool<Loader: PoolDataLoader = DataLoader> {
    sync_swap_with_sim:     bool,
    initial_ticks_per_side: u16,
    pub data_loader:        Loader,
    pub token0:             Address,
    pub token0_decimals:    u8,
    pub token1:             Address,
    pub token1_decimals:    u8,
    pub liquidity:          u128,
    pub liquidity_net:      i128,
    pub sqrt_price:         U256,
    pub fee:                u32,
    pub tick:               i32,
    pub tick_spacing:       i32,
    pub tick_bitmap:        HashMap<i16, U256>,
    pub ticks:              HashMap<i32, TickInfo>
}

impl<Loader> EnhancedUniswapPool<Loader>
where
    Loader: PoolDataLoader + Default
{
    pub fn new(data_loader: Loader, initial_ticks_per_side: u16) -> Self {
        Self {
            initial_ticks_per_side,
            sync_swap_with_sim: false,
            data_loader,
            ..Default::default()
        }
    }

    pub fn is_sync_swap_with_sim(&self) -> bool {
        self.sync_swap_with_sim
    }

    pub fn initial_ticks_per_side(&self) -> u16 {
        self.initial_ticks_per_side
    }

    pub fn data_loader(&self) -> Loader {
        self.data_loader.clone()
    }

    pub async fn pool_data_for_block<T: Transport + Clone>(
        &self,
        block_number: BlockNumber,
        provider: Arc<impl Provider>
    ) -> Result<PoolData, PoolError> {
        self.data_loader
            .load_pool_data(Some(block_number), provider)
            .await
    }

    pub fn fetch_lowest_tick(&self) -> i32 {
        *self.ticks.keys().min().unwrap()
    }

    pub fn fetch_highest_tick(&self) -> i32 {
        *self.ticks.keys().max().unwrap()
    }

    pub fn fetch_pool_snapshot(&self) -> Result<(Address, Address, PoolSnapshot), PoolError> {
        if !self.data_is_populated() {
            return Err(PoolError::PoolNotInitialized);
        }

        // Get the information for our current position
        let current_liquidity = self.liquidity;
        let current_tick = self.tick;
        let (less_than, more_than): (Vec<_>, Vec<_>) = self
            .ticks
            .iter()
            .sorted_unstable_by(|(tick_a, _), (tick_b, _)| tick_a.cmp(tick_b))
            .partition(|t| *t.0 <= current_tick);

        let current_range = LiqRange::new(
            // Because they're already sorted low to high - we want the highest low and the lowest
            // high
            *less_than.last().expect("No lower bound to current range").0,
            *more_than
                .first()
                .expect("No upper bound to corrent range")
                .0,
            current_liquidity
        )
        .unwrap();

        let mut tracked_liq: i128 = current_liquidity.saturating_into();
        let upper_ranges = more_than
            .iter()
            // Sort low to high
            .sorted_unstable_by(|(tick_a, _), (tick_b, _)| tick_a.cmp(tick_b))
            .tuple_windows()
            .map(|((low_tick, low_tick_data), (high_tick, _))| {
                // We're walking upwards so we add the low tick's net
                tracked_liq += low_tick_data.liquidity_net;
                // ensure everything is spaced properly
                assert!(low_tick.rem(self.tick_spacing) == 0, "Lower tick not aligned");
                assert!(high_tick.rem(self.tick_spacing) == 0, "Upper tick not aligned");
                assert!(
                    tracked_liq >= 0,
                    "Liquidity dropped below zero {} - {}",
                    low_tick,
                    high_tick
                );
                LiqRange::new(**low_tick, **high_tick, tracked_liq.unsigned_abs()).unwrap()
            })
            .collect::<Vec<_>>();

        // Reset our tracked liquidity
        tracked_liq = current_liquidity.saturating_into();
        // Iterate downwards now
        let lower_ranges = less_than
            .iter()
            // We want to walk our ranges high to low, so we reverse
            .rev()
            .tuple_windows()
            .map(|((high_tick, high_tick_data), (low_tick, _))| {
                // We're walking downwards now so we subtract the upper tick's net
                tracked_liq -= high_tick_data.liquidity_net;
                // ensure everything is spaced properly
                assert!(low_tick.rem(self.tick_spacing) == 0, "Lower tick not aligned");
                assert!(high_tick.rem(self.tick_spacing) == 0, "Upper tick not aligned");
                assert!(
                    tracked_liq >= 0,
                    "Liquidity dropped below zero {} - {}",
                    low_tick,
                    high_tick
                );
                LiqRange::new(**low_tick, **high_tick, tracked_liq.unsigned_abs()).unwrap()
            })
            .collect::<Vec<_>>();

        // Put them all together - the PoolSnapshot constructor will sort this
        let liq_ranges =
            [[current_range].as_slice(), upper_ranges.as_slice(), lower_ranges.as_slice()].concat();

        Ok((
            self.token0,
            self.token1,
            PoolSnapshot::new(self.tick_spacing, liq_ranges, self.sqrt_price.into())?
        ))
    }

    pub async fn initialize(
        &mut self,
        block_number: Option<BlockNumber>,
        provider: Arc<impl Provider>
    ) -> Result<(), PoolError> {
        tracing::trace!(?block_number, "populating pool data");
        self.populate_data(block_number, provider.clone()).await?;
        tracing::trace!(?block_number, "populated pool data");
        self.sync_ticks(block_number, provider.clone()).await?;
        tracing::trace!(?block_number, "synced pool ticks");
        Ok(())
    }

    pub async fn update_to_block<P: Provider>(
        &mut self,
        block_number: Option<BlockNumber>,
        provider: Arc<P>
    ) -> Result<(), PoolError> {
        self.populate_data(block_number, provider.clone()).await?;
        self.sync_ticks(block_number, provider.clone()).await?;
        Ok(())
    }

    pub fn set_sim_swap_sync(&mut self, sync_swap_with_sim: bool) {
        self.sync_swap_with_sim = sync_swap_with_sim;
    }

    pub fn address(&self) -> PoolId {
        self.data_loader.address()
    }

    async fn get_tick_data_batch_request<P: Provider>(
        &self,
        tick_start: I24,
        zero_for_one: bool,
        num_ticks: u16,
        block_number: Option<BlockNumber>,
        provider: Arc<P>
    ) -> Result<(Vec<TickData>, U256), PoolError> {
        let (tick_data, block_number) = self
            .data_loader
            .load_tick_data(
                tick_start,
                zero_for_one,
                num_ticks,
                i32_to_i24(self.tick_spacing)?,
                block_number,
                provider.clone()
            )
            .await?;

        Ok((tick_data, block_number))
    }

    pub fn apply_ticks(&mut self, mut fetched_ticks: Vec<TickData>) {
        fetched_ticks.sort_by_key(|k| k.tick);

        fetched_ticks
            .into_iter()
            .filter(|tick| tick.initialized)
            .for_each(|tick| {
                self.ticks.insert(
                    tick.tick.as_i32(),
                    TickInfo {
                        initialized:     tick.initialized,
                        liquidity_gross: tick.liquidityGross,
                        liquidity_net:   tick.liquidityNet
                    }
                );
                self.flip_tick(tick.tick.as_i32(), self.tick_spacing);
            });
    }

    pub async fn load_more_ticks<P: Provider>(
        &self,
        tick_data: TickRangeToLoad,
        block_number: Option<BlockNumber>,
        provider: Arc<P>
    ) -> Result<Vec<TickData>, PoolError> {
        Ok(self
            .get_tick_data_batch_request(
                i32_to_i24(tick_data.start_tick)?,
                tick_data.zfo,
                tick_data.tick_count,
                block_number,
                provider
            )
            .await?
            .0)
    }

    async fn sync_ticks<P: Provider>(
        &mut self,
        block_number: Option<u64>,
        provider: Arc<P>
    ) -> Result<(), PoolError> {
        if !self.data_is_populated() {
            return Err(PoolError::PoolAlreadyInitialized);
        }

        self.ticks.clear();
        self.tick_bitmap.clear();

        tracing::info!(?self.token0, ?self.token1,?self.tick, ?self.tick_spacing, ?self.liquidity,?self.liquidity_net);

        let extra = (self.tick.rem(self.tick_spacing) == 0i32) as u8;

        let total_ticks_to_fetch = self.initial_ticks_per_side * 2;
        // current tick when loaded (init tick) - (half total tics * spacing);

        let start_tick = self.tick
            - (total_ticks_to_fetch.div_ceil(2) as i32 * self.tick_spacing)
            - (extra as i32);

        let end_tick = start_tick + (self.tick_spacing as u16 * total_ticks_to_fetch) as i32;
        tracing::info!(?start_tick, ?end_tick);

        // Fetch ticks in batches of TICKS_PER_BATCH
        let mut fetched_ticks = Vec::new();
        let mut current_tick = start_tick;
        let remaining_ticks = total_ticks_to_fetch;

        for batch_idx in 0..(remaining_ticks.div_ceil(TICKS_PER_BATCH as u16)) {
            let batch_size = std::cmp::min(
                TICKS_PER_BATCH as u16,
                remaining_ticks - (batch_idx * TICKS_PER_BATCH as u16)
            );

            if batch_size == 0 {
                break;
            }

            tracing::debug!(?current_tick, ?batch_size, batch_idx, "Fetching tick batch");

            let batch_ticks = self
                .get_tick_data_batch_request(
                    i32_to_i24(current_tick)?,
                    false,
                    batch_size,
                    block_number,
                    provider.clone()
                )
                .await?
                .0;

            // Update current_tick for the next batch
            if !batch_ticks.is_empty() {
                // Move to the next tick after the last one in this batch
                current_tick += (self.tick_spacing as u16 * batch_size) as i32;
            }

            fetched_ticks.extend(batch_ticks);
        }

        fetched_ticks.sort_by_key(|k| k.tick);

        fetched_ticks
            .into_iter()
            .filter(|tick| tick.initialized)
            .for_each(|tick| {
                tracing::trace!(
                    initialized = tick.initialized,
                    liq_gross = tick.liquidityGross,
                    liq_net = tick.liquidityNet,
                    tick = tick.tick.as_i32(),
                    "Inserting tick"
                );
                self.ticks.insert(
                    tick.tick.as_i32(),
                    TickInfo {
                        initialized:     tick.initialized,
                        liquidity_gross: tick.liquidityGross,
                        liquidity_net:   tick.liquidityNet
                    }
                );
                self.flip_tick(tick.tick.as_i32(), self.tick_spacing);
            });

        Ok(())
    }

    pub fn calculate_price_unshifted(&self, sqrt_price_limit_x96: U256) -> f64 {
        let tick =
            uniswap_v3_math::tick_math::get_tick_at_sqrt_ratio(sqrt_price_limit_x96).unwrap();

        let shift = self.token0_decimals as i8 - self.token1_decimals as i8;
        // flipped to scale them properly with the token spacing
        match shift.cmp(&0) {
            Ordering::Less => 1.0001_f64.powi(tick) * 10_f64.powi(-shift as i32),
            Ordering::Greater => 1.0001_f64.powi(tick) / 10_f64.powi(shift as i32),
            Ordering::Equal => 1.0001_f64.powi(tick)
        }
    }

    pub fn calculate_price(&self) -> f64 {
        let tick = uniswap_v3_math::tick_math::get_tick_at_sqrt_ratio(self.sqrt_price).unwrap();
        let shift = self.token0_decimals as i8 - self.token1_decimals as i8;
        match shift.cmp(&0) {
            Ordering::Less => 1.0001_f64.powi(tick) / 10_f64.powi(-shift as i32),
            Ordering::Greater => 1.0001_f64.powi(tick) * 10_f64.powi(shift as i32),
            Ordering::Equal => 1.0001_f64.powi(tick)
        }
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
    fn _simulate_swap(
        &self,
        token_in: Address,
        amount_specified: I256,
        sqrt_price_limit_x96: Option<U256>
    ) -> Result<SwapResult, SwapSimulationError> {
        if amount_specified.is_zero() {
            return Err(SwapSimulationError::ZeroAmountSpecified);
        }

        let zero_for_one = token_in == self.token0;
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
            tracing::warn!(?zero_for_one, ?sqrt_price_limit_x96, ?self.sqrt_price);
            return Err(SwapSimulationError::InvalidSqrtPriceLimit);
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
                                if zero_for_one { -info.liquidity_net } else { info.liquidity_net }
                            })
                            .unwrap_or_default();

                    liquidity = if liquidity_net < 0 {
                        liquidity
                            .checked_sub(liquidity_net.unsigned_abs())
                            .ok_or(SwapSimulationError::LiquidityUnderflow)?
                    } else {
                        liquidity + (liquidity_net.unsigned_abs())
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

        Ok(SwapResult { amount0, amount1, liquidity, sqrt_price_x_96, tick })
    }

    pub fn simulate_swap(
        &self,
        token_in: Address,
        amount_specified: I256,
        sqrt_price_limit_x96: Option<U256>
    ) -> Result<(I256, I256), SwapSimulationError> {
        let swap_result = self._simulate_swap(token_in, amount_specified, sqrt_price_limit_x96)?;
        Ok((swap_result.amount0, swap_result.amount1))
    }

    pub fn simulate_swap_mut(
        &mut self,
        token_in: Address,
        amount_specified: I256,
        sqrt_price_limit_x96: Option<U256>
    ) -> Result<(I256, I256), SwapSimulationError> {
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
            self._sync_from_swap_log(log)
        }
    }

    fn sync_swap_with_sim(&mut self, log: Log) -> Result<(), PoolError> {
        let swap_event = Loader::decode_swap_event(&log)?;

        tracing::trace!(pool_tick = ?self.tick, pool_price = ?self.sqrt_price, pool_liquidity = ?self.liquidity, pool_address = ?self.data_loader.address(), "pool before");
        tracing::debug!(swap_tick=swap_event.tick, swap_price=?swap_event.sqrt_price_x96, swap_liquidity=?swap_event.liquidity, swap_amount0=?swap_event.amount0, swap_amount1=?swap_event.amount1, "swap event");

        let combinations = [
            (self.token1, swap_event.amount1),
            (self.token0, swap_event.amount0),
            (self.token0, swap_event.amount1),
            (self.token1, swap_event.amount0)
        ];

        let mut simulation_failed = true;
        for (token_in, amount_in) in combinations.iter() {
            let sqrt_price_limit_x96 = Some(U256::from(swap_event.sqrt_price_x96));
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
                pool_address = ?self.data_loader.address(),
                pool_price = ?self.sqrt_price,
                pool_liquidity = ?self.liquidity,
                pool_tick = ?self.tick,
                swap_price = ?swap_event.sqrt_price_x96,
                swap_tick = swap_event.tick,
                swap_liquidity = ?swap_event.liquidity,
                swap_amount0 = ?swap_event.amount0,
                swap_amount1 = ?swap_event.amount1,
                "Swap simulation failed"
            );
            return Err(PoolError::SwapSimulationFailed);
        } else {
            tracing::trace!(pool_tick = ?self.tick, pool_price = ?self.sqrt_price, pool_liquidity = ?self.liquidity, pool_address = ?self.data_loader.address(), "pool after");
        }

        Ok(())
    }

    pub fn sync_from_log(&mut self, log: Log) -> Result<(), PoolError> {
        if Loader::is_swap_event(&log) {
            self._sync_from_swap_log(log)?;
        } else if Loader::is_modify_position_event(&log) {
            self.sync_from_modify_position(log)?;
        } else {
            Err(PoolError::InvalidEventSignature(log.topics().to_vec()))?
        }

        Ok(())
    }

    pub fn sync_from_modify_position(&mut self, log: Log) -> Result<(), PoolError> {
        let modify_position_event = Loader::decode_modify_position_event(&log)?;
        let ModifyPositionEvent { tick_lower, tick_upper, liquidity_delta, .. } =
            modify_position_event;

        self.update_position(tick_lower, tick_upper, liquidity_delta);

        if liquidity_delta != 0 && self.tick > tick_lower && self.tick < tick_upper {
            self.liquidity = if liquidity_delta < 0 {
                self.liquidity - (liquidity_delta.unsigned_abs())
            } else {
                // > 0 so we can just
                self.liquidity + (liquidity_delta.unsigned_abs())
            }
        }

        tracing::debug!(?modify_position_event, address = ?self.data_loader.address(), sqrt_price = ?self.sqrt_price, liquidity = ?self.liquidity, tick = ?self.tick, "modify position event");

        Ok(())
    }

    pub fn _sync_from_swap_log(&mut self, log: Log) -> Result<(), PoolError> {
        let swap_event = Loader::decode_swap_event(&log)?;

        self.sqrt_price = U256::from(swap_event.sqrt_price_x96);
        self.liquidity = swap_event.liquidity;
        self.tick = swap_event.tick;

        tracing::debug!(?swap_event, address = ?self.data_loader.address(), sqrt_price = ?self.sqrt_price, liquidity = ?self.liquidity, tick = ?self.tick, "swap event");

        Ok(())
    }

    pub async fn populate_data<P: Provider>(
        &mut self,
        block_number: Option<u64>,
        provider: Arc<P>
    ) -> Result<(), PoolError> {
        let pool_data = self
            .data_loader
            .load_pool_data(block_number, provider)
            .await?;

        self.token0 = pool_data.tokenA;
        self.token0_decimals = pool_data.tokenADecimals;
        self.token1 = pool_data.tokenB;
        self.token1_decimals = pool_data.tokenBDecimals;
        self.liquidity = pool_data.liquidity;
        self.sqrt_price = U256::from(pool_data.sqrtPrice);
        self.tick = pool_data.tick.as_i32();
        self.tick_spacing = pool_data.tickSpacing.as_i32();
        let mut bytes = [0u8; 4];
        bytes[..3].copy_from_slice(&pool_data.fee.to_le_bytes::<3>());
        self.fee = u32::from_le_bytes(bytes);
        self.liquidity_net = pool_data.liquidityNet;
        Ok(())
    }

    pub fn data_is_populated(&self) -> bool {
        !(self.token0.is_zero() || self.token1.is_zero())
    }

    pub(crate) fn update_position(
        &mut self,
        tick_lower: i32,
        tick_upper: i32,
        liquidity_delta: i128
    ) {
        let mut flipped_lower = false;
        let mut flipped_upper = false;

        if liquidity_delta != 0 {
            flipped_lower = self.update_tick(tick_lower, liquidity_delta, false);
            flipped_upper = self.update_tick(tick_upper, liquidity_delta, true);
            if flipped_lower {
                self.flip_tick(tick_lower, self.tick_spacing);
            }
            if flipped_upper {
                self.flip_tick(tick_upper, self.tick_spacing);
            }
        }

        if liquidity_delta < 0 {
            if flipped_lower {
                self.ticks.remove(&tick_lower);
            }

            if flipped_upper {
                self.ticks.remove(&tick_upper);
            }
        }
    }

    pub fn update_tick(&mut self, tick: i32, liquidity_delta: i128, upper: bool) -> bool {
        let info = match self.ticks.get_mut(&tick) {
            Some(info) => info,
            None => {
                self.ticks.insert(tick, TickInfo::default());
                self.ticks
                    .get_mut(&tick)
                    .expect("Tick does not exist in ticks")
            }
        };

        let liquidity_gross_before = info.liquidity_gross;

        let liquidity_gross_after = if liquidity_delta < 0 {
            liquidity_gross_before.saturating_sub(liquidity_delta.unsigned_abs())
        } else {
            liquidity_gross_before + (liquidity_delta.unsigned_abs())
        };

        // we do not need to check if liqudity_gross_after > maxLiquidity because we are
        // only calling update tick on a burn or mint log. this should already
        // be validated when a log is
        let flipped = (liquidity_gross_after == 0) != (liquidity_gross_before == 0);

        if liquidity_gross_before == 0 {
            info.initialized = true;
        }

        info.liquidity_gross = liquidity_gross_after;

        info.liquidity_net = if upper {
            info.liquidity_net - liquidity_delta
        } else {
            info.liquidity_net + liquidity_delta
        };

        flipped
    }

    pub fn flip_tick(&mut self, tick: i32, tick_spacing: i32) {
        let (word_pos, bit_pos) = uniswap_v3_math::tick_bitmap::position(tick / tick_spacing);
        let mask = U256::from(1) << bit_pos;

        if let Some(word) = self.tick_bitmap.get_mut(&word_pos) {
            *word ^= mask;
        } else {
            self.tick_bitmap.insert(word_pos, mask);
        }
    }

    pub fn get_token_out(&self, token_in: Address) -> Address {
        if self.token0 == token_in { self.token1 } else { self.token0 }
    }

    pub fn calculate_word_pos_bit_pos(&self, compressed: i32) -> (i16, u8) {
        uniswap_v3_math::tick_bitmap::position(compressed)
    }

    #[cfg(test)]
    pub fn set_sqrt_price_x96(&mut self, price: u128) {
        self.sqrt_price = U256::from(price);
    }

    #[cfg(test)]
    pub fn get_sqrt_price_x96(&self) -> u128 {
        self.sqrt_price.to()
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
    #[error("Invalid signature: [{}]", .0.iter().map(|b| format!("0x{}", hex::encode(b))).collect::<Vec<_>>().join(", "))]
    InvalidEventSignature(Vec<B256>),
    #[error("Swap simulation failed")]
    SwapSimulationFailed,
    #[error("Pool already initialized")]
    PoolAlreadyInitialized,
    #[error("Pool is not initialized")]
    PoolNotInitialized,
    #[error(transparent)]
    SwapSimulationError(#[from] SwapSimulationError),
    #[error(transparent)]
    AlloyContractError(#[from] alloy::contract::Error),
    #[error(transparent)]
    AlloySolTypeError(#[from] alloy::sol_types::Error),
    #[error(transparent)]
    ConversionError(#[from] ConversionError),
    #[error(transparent)]
    Eyre(#[from] eyre::Error)
}

#[cfg(test)]
mod tests {
    use std::sync::Once;

    use tracing_subscriber::{EnvFilter, fmt};
    use uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick;

    use super::*;
    use crate::uniswap::pool_data_loader;

    static INIT: Once = Once::new();

    fn setup_tracing() {
        INIT.call_once(|| {
            let _ = fmt()
                .with_env_filter(
                    EnvFilter::from_default_env()
                        .add_directive("uniswap_v4=debug".parse().unwrap())
                        .add_directive("angstrom_types=debug".parse().unwrap())
                        .add_directive("test=debug".parse().unwrap())
                )
                .try_init();
        });
    }

    #[derive(Debug, Clone, Default)]
    struct MockLoader;

    impl PoolDataLoader for MockLoader {
        async fn load_tick_data<P: Provider>(
            &self,
            _: I24,
            _: bool,
            _: u16,
            _: I24,
            _: Option<BlockNumber>,
            _: Arc<P>
        ) -> Result<(Vec<TickData>, U256), PoolError> {
            unimplemented!()
        }

        async fn load_pool_data<P: Provider>(
            &self,
            _: Option<BlockNumber>,
            _: Arc<P>
        ) -> Result<PoolData, PoolError> {
            unimplemented!()
        }

        fn address(&self) -> PoolId {
            unimplemented!()
        }

        fn group_logs(_: Vec<Log>) -> HashMap<PoolId, Vec<Log>> {
            unimplemented!()
        }

        fn event_signatures() -> Vec<B256> {
            unimplemented!()
        }

        fn is_swap_event(_: &Log) -> bool {
            unimplemented!()
        }

        fn is_modify_position_event(_: &Log) -> bool {
            unimplemented!()
        }

        fn decode_swap_event(_: &Log) -> Result<pool_data_loader::SwapEvent, PoolError> {
            unimplemented!()
        }

        fn decode_modify_position_event(_: &Log) -> Result<ModifyPositionEvent, PoolError> {
            unimplemented!()
        }
    }

    fn setup_basic_pool() -> EnhancedUniswapPool<MockLoader> {
        let mut pool = EnhancedUniswapPool::new(MockLoader, 10);
        pool.token0 = Address::from_slice(&[1u8; 20]);
        pool.token1 = Address::from_slice(&[2u8; 20]);
        pool.token0_decimals = 18;
        pool.token1_decimals = 18;
        pool.liquidity = 1_000_000;
        pool.sqrt_price = U256::from(1004968906420141727126888u128);
        pool.fee = 3000;
        pool.tick = 1000;
        pool.tick_spacing = 60;
        pool
    }

    #[test]
    fn test_get_token_out() {
        setup_tracing();
        let pool = setup_basic_pool();

        let token_out = pool.get_token_out(pool.token0);
        assert_eq!(token_out, pool.token1);

        let token_out = pool.get_token_out(pool.token1);
        assert_eq!(token_out, pool.token0);
    }

    #[test]
    fn test_calculate_price() {
        setup_tracing();
        let pool = setup_basic_pool();
        let price = pool.calculate_price();
        assert!(price > 0.0);
    }

    #[test]
    fn test_update_position() {
        setup_tracing();
        let mut pool = setup_basic_pool();

        // First add the ticks to the pool
        pool.ticks.insert(-120, TickInfo::default());
        pool.ticks.insert(120, TickInfo::default());

        // Test Case 1: Add liquidity
        pool.update_position(-120, 120, 1000);

        // Verify lower tick
        let lower_tick = pool.ticks.get(&-120).unwrap();
        assert_eq!(lower_tick.liquidity_gross, 1000u128);
        assert_eq!(lower_tick.liquidity_net, 1000);
        assert!(lower_tick.initialized);

        // Verify upper tick
        let upper_tick = pool.ticks.get(&120).unwrap();
        assert_eq!(upper_tick.liquidity_gross, 1000u128);
        assert_eq!(upper_tick.liquidity_net, -1000);
        assert!(upper_tick.initialized);

        // Test Case 2: Add more liquidity to same position
        pool.update_position(-120, 120, 500);

        let lower_tick = pool.ticks.get(&-120).unwrap();
        assert_eq!(lower_tick.liquidity_gross, 1500u128);
        assert_eq!(lower_tick.liquidity_net, 1500);

        let upper_tick = pool.ticks.get(&120).unwrap();
        assert_eq!(upper_tick.liquidity_gross, 1500u128);
        assert_eq!(upper_tick.liquidity_net, -1500);

        // Test Case 3: Remove partial liquidity
        pool.update_position(-120, 120, -500);

        let lower_tick = pool.ticks.get(&-120).unwrap();
        assert_eq!(lower_tick.liquidity_gross, 1000u128);
        assert_eq!(lower_tick.liquidity_net, 1000);

        let upper_tick = pool.ticks.get(&120).unwrap();
        assert_eq!(upper_tick.liquidity_gross, 1000u128);
        assert_eq!(upper_tick.liquidity_net, -1000);

        // Test Case 4: Remove all remaining liquidity
        pool.update_position(-120, 120, -1000);

        // Ticks should be removed after flipping to zero liquidity
        assert!(!pool.ticks.contains_key(&-120));
        assert!(!pool.ticks.contains_key(&120));
    }

    #[test]
    fn test_update_position_overlapping_ranges() {
        setup_tracing();
        let mut pool = setup_basic_pool();

        // Add ticks to the pool
        pool.ticks.insert(-120, TickInfo::default());
        pool.ticks.insert(-60, TickInfo::default());
        pool.ticks.insert(60, TickInfo::default());
        pool.ticks.insert(120, TickInfo::default());

        // Create overlapping ranges
        pool.update_position(-120, 120, 1000); // Outer range
        pool.update_position(-60, 60, 500); // Inner range

        // Check outer range ticks
        let tick_minus_120 = pool.ticks.get(&-120).unwrap();
        assert_eq!(tick_minus_120.liquidity_gross, 1000u128);
        assert_eq!(tick_minus_120.liquidity_net, 1000);

        let tick_120 = pool.ticks.get(&120).unwrap();
        assert_eq!(tick_120.liquidity_gross, 1000u128);
        assert_eq!(tick_120.liquidity_net, -1000);

        // Check inner range ticks
        let tick_minus_60 = pool.ticks.get(&-60).unwrap();
        assert_eq!(tick_minus_60.liquidity_gross, 500u128);
        assert_eq!(tick_minus_60.liquidity_net, 500);

        let tick_60 = pool.ticks.get(&60).unwrap();
        assert_eq!(tick_60.liquidity_gross, 500u128);
        assert_eq!(tick_60.liquidity_net, -500);

        // Remove inner range
        pool.update_position(-60, 60, -500);

        // Verify outer range remains intact
        assert_eq!(pool.ticks.get(&-120).unwrap().liquidity_gross, 1000u128);
        assert_eq!(pool.ticks.get(&120).unwrap().liquidity_gross, 1000u128);
    }

    #[test]
    fn test_flip_tick() {
        setup_tracing();
        let mut pool = setup_basic_pool();

        // Flip tick
        pool.flip_tick(60, 60);
        let (word_pos, bit_pos) = pool.calculate_word_pos_bit_pos(1);
        let mask = U256::from(1) << bit_pos;
        assert_eq!(pool.tick_bitmap.get(&word_pos), Some(&mask));

        // Flip again should clear
        pool.flip_tick(60, 60);
        assert_eq!(pool.tick_bitmap.get(&word_pos), Some(&U256::ZERO));
    }

    #[test]
    fn test_data_is_populated() {
        setup_tracing();
        let pool = setup_basic_pool();
        assert!(pool.data_is_populated());

        let empty_pool = EnhancedUniswapPool::<MockLoader>::default();
        assert!(!empty_pool.data_is_populated());
    }

    #[test]
    fn test_simulate_swap_invalid_cases() {
        setup_tracing();
        let pool = setup_basic_pool();

        // Test zero amount
        let result = pool.simulate_swap(pool.token0, I256::ZERO, None);
        assert!(matches!(result, Err(SwapSimulationError::ZeroAmountSpecified)));

        // Test invalid sqrt price limit
        let invalid_price = MAX_SQRT_RATIO;
        let result =
            pool.simulate_swap(pool.token0, I256::try_from(1000i32).unwrap(), Some(invalid_price));
        assert!(matches!(result, Err(SwapSimulationError::InvalidSqrtPriceLimit)));
    }

    #[test]
    fn test_fetch_pool_snapshot() {
        setup_tracing();
        let mut pool = setup_basic_pool();

        // Add some ticks
        pool.update_position(-180, -60, 1000);
        pool.update_position(-60, 0, 2000);
        pool.update_position(0, 60, 3000);
        // set the sqrt_liq at -120
        let sqrt = get_sqrt_ratio_at_tick(-60).unwrap();
        pool.sqrt_price = sqrt;
        pool.tick = -60;

        let result = pool.fetch_pool_snapshot();
        assert!(result.is_ok(), "{:?}", result);

        let (token_a, token_b, snapshot) = result.unwrap();
        assert_eq!(token_a, pool.token0);
        assert_eq!(token_b, pool.token1);
        assert!(!snapshot.ranges.is_empty());
        println!("Snapshot: {:#?}", snapshot);
    }
}
