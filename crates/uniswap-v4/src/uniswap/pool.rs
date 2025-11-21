use std::{cmp::Ordering, collections::HashMap, fmt::Debug, ops::Rem, sync::Arc};

// use std::iter::
use alloy::{
    hex,
    network::Network,
    primitives::{Address, B256, BlockNumber, I256, U256, aliases::I24},
    providers::Provider,
    transports::Transport
};
use angstrom_types::{
    primitive::{PoolId, TickInfo},
    uni_structure::{BaselinePoolState, liquidity_base::BaselineLiquidity}
};
use itertools::Itertools;
use thiserror::Error;
use uniswap_v3_math::{
    error::UniswapV3MathError,
    tick_math::{MAX_SQRT_RATIO, MAX_TICK, MIN_SQRT_RATIO, MIN_TICK}
};

use super::{pool_data_loader::PoolData, pool_manager::TickRangeToLoad};
use crate::uniswap::{
    ConversionError, i32_to_i24,
    pool_data_loader::{DataLoader, PoolDataLoader, TickData}
};

#[derive(Default)]
pub struct SwapResult {
    pub amount0:         I256,
    pub amount1:         I256,
    pub sqrt_price_x_96: U256,
    pub liquidity:       u128,
    pub tick:            i32
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
    pub block_number:       u64,
    pub liquidity:          u128,
    pub liquidity_net:      i128,
    pub sqrt_price_x96:     U256,
    pub book_fee:           u32,
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
        *self.ticks.keys().min().expect("no lowest tick")
    }

    pub fn fetch_highest_tick(&self) -> i32 {
        *self.ticks.keys().max().expect("no highest tick")
    }

    pub fn block_number(&self) -> u64 {
        self.block_number
    }

    pub fn fetch_pool_snapshot(&self) -> Result<(Address, Address, BaselinePoolState), PoolError> {
        if !self.data_is_populated() {
            return Err(PoolError::PoolNotInitialized);
        }

        Ok((
            self.token0,
            self.token1,
            BaselinePoolState::new(
                BaselineLiquidity::new(
                    self.tick_spacing,
                    self.tick,
                    self.sqrt_price_x96.into(),
                    self.liquidity,
                    self.ticks.clone(),
                    self.tick_bitmap.clone()
                ),
                self.block_number,
                self.book_fee
            )
        ))
    }

    pub async fn initialize<P: Provider<N>, N: Network>(
        &mut self,
        block_number: Option<BlockNumber>,
        provider: Arc<P>
    ) -> Result<(), PoolError> {
        tracing::trace!(?block_number, "populating pool data");
        self.populate_data(block_number, provider.clone()).await?;
        tracing::trace!(?block_number, "populated pool data");
        self.sync_ticks(block_number, provider.clone()).await?;
        tracing::trace!(?block_number, "synced pool ticks");
        Ok(())
    }

    pub async fn update_to_block<P: Provider<N>, N: Network>(
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

    pub fn public_address(&self) -> PoolId {
        self.data_loader.public_address()
    }

    pub fn private_address(&self) -> PoolId {
        self.data_loader.private_address()
    }

    async fn get_tick_data_batch_request<P: Provider<N>, N: Network>(
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
            .unique()
            .filter(|f| f.initialized)
            .map(|t| (!t.initialized, t))
            .for_each(|(is_edge, tick)| {
                self.ticks.insert(
                    tick.tick.as_i32(),
                    TickInfo {
                        initialized:     tick.initialized,
                        liquidity_gross: tick.liquidityGross,
                        liquidity_net:   tick.liquidityNet
                    }
                );

                // only flip initalized ticks.
                if !is_edge {
                    self.flip_tick_if_not_init(tick.tick.as_i32(), self.tick_spacing);
                }
            });
    }

    pub async fn load_more_ticks<P: Provider<N>, N: alloy::network::Network>(
        &mut self,
        tick_data: TickRangeToLoad,
        block_number: Option<BlockNumber>,
        provider: Arc<P>
    ) -> Result<(), PoolError> {
        let ticks = self
            .get_tick_data_batch_request(
                i32_to_i24(tick_data.start_tick)?,
                tick_data.zfo,
                tick_data.tick_count,
                block_number,
                provider
            )
            .await?
            .0;

        // if we are zero for one, means that we need a new end tick.
        self.apply_ticks(ticks);

        Ok(())
    }

    async fn sync_ticks<P: Provider<N>, N: alloy::network::Network>(
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

        let (asks, bids) = futures::join!(
            self.load_ticks_in_direction(provider.clone(), block_number, true),
            self.load_ticks_in_direction(provider.clone(), block_number, false),
        );
        let (_, _, asks) = asks?;
        self.apply_ticks(asks);

        let (_, _, bids) = bids?;

        self.apply_ticks(bids);

        Ok(())
    }

    async fn load_ticks_in_direction<P: Provider<N>, N: alloy::network::Network>(
        &self,
        provider: Arc<P>,
        block_number: Option<u64>,
        direction: bool
    ) -> Result<(i32, i32, Vec<TickData>), PoolError> {
        let extra = (self.tick.rem(self.tick_spacing) == 0i32) as u8;
        // true = zfo
        let adjustment = if direction {
            self.tick_spacing * self.initial_ticks_per_side as i32 - extra as i32
        } else {
            -(self.tick_spacing * self.initial_ticks_per_side as i32 - extra as i32)
        };
        let start_tick = self.tick + adjustment;

        let mut fetched_ticks = Vec::new();
        let mut current_tick = start_tick;

        for _ in 0..(self.initial_ticks_per_side.div_ceil(TICKS_PER_BATCH as u16)) {
            let batch_ticks = self
                .get_tick_data_batch_request(
                    i32_to_i24(current_tick)?,
                    direction,
                    TICKS_PER_BATCH as u16,
                    block_number,
                    provider.clone()
                )
                .await?
                .0;

            let Some(next_tick) = (if direction {
                batch_ticks
                    .iter()
                    .min_by_key(|a| a.tick)
                    .map(|k| k.tick.as_i32())
            } else {
                batch_ticks
                    .iter()
                    .max_by_key(|a| a.tick)
                    .map(|k| k.tick.as_i32())
            }) else {
                break;
            };

            let len = batch_ticks.len();
            current_tick = next_tick;
            fetched_ticks.extend(batch_ticks);
            if len != TICKS_PER_BATCH {
                break;
            }
        }

        Ok((start_tick, current_tick, fetched_ticks))
    }

    pub fn calculate_price_unshifted(&self, sqrt_price_limit_x96: U256) -> f64 {
        let tick =
            uniswap_v3_math::tick_math::get_tick_at_sqrt_ratio(sqrt_price_limit_x96).unwrap();

        let shift = self.token0_decimals as i8 - self.token1_decimals as i8;
        match shift.cmp(&0) {
            Ordering::Less => 1.0001_f64.powi(tick) / 10_f64.powi(-shift as i32),
            Ordering::Greater => 1.0001_f64.powi(tick) * 10_f64.powi(shift as i32),
            Ordering::Equal => 1.0001_f64.powi(tick)
        }
    }

    pub fn calculate_price(&self) -> f64 {
        let tick = uniswap_v3_math::tick_math::get_tick_at_sqrt_ratio(self.sqrt_price_x96).unwrap();
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
    pub fn _simulate_swap(
        &self,
        token_in: Address,
        amount_specified: I256,
        sqrt_price_limit_x96: Option<U256>,
        with_fee: bool
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
            && (sqrt_price_limit_x96 >= self.sqrt_price_x96
                || sqrt_price_limit_x96 <= MIN_SQRT_RATIO))
            || (!zero_for_one
                && (sqrt_price_limit_x96 <= self.sqrt_price_x96
                    || sqrt_price_limit_x96 >= MAX_SQRT_RATIO))
        {
            tracing::warn!(?zero_for_one, ?sqrt_price_limit_x96, ?self.sqrt_price_x96);
            return Err(SwapSimulationError::InvalidSqrtPriceLimit);
        }

        let mut amount_specified_remaining = amount_specified;
        let mut amount_calculated = I256::ZERO;
        let mut sqrt_price_x_96 = self.sqrt_price_x96;
        let mut tick = self.tick;
        let mut liquidity = self.liquidity;
        let fee = if with_fee { self.book_fee } else { Default::default() };

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
                    fee
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
        let swap_result =
            self._simulate_swap(token_in, amount_specified, sqrt_price_limit_x96, false)?;
        Ok((swap_result.amount0, swap_result.amount1))
    }

    pub async fn populate_data<P: Provider<N>, N: alloy::network::Network>(
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
        self.sqrt_price_x96 = U256::from(pool_data.sqrtPrice);
        self.tick = pool_data.tick.as_i32();
        self.tick_spacing = pool_data.tickSpacing.as_i32();
        self.book_fee = self.data_loader.pool_fee();
        self.liquidity_net = pool_data.liquidityNet;
        Ok(())
    }

    pub fn data_is_populated(&self) -> bool {
        !(self.token0.is_zero() || self.token1.is_zero())
    }

    #[cfg(test)]
    pub fn update_position(&mut self, tick_lower: i32, tick_upper: i32, liquidity_delta: i128) {
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

    pub fn flip_tick_if_not_init(&mut self, tick: i32, tick_spacing: i32) {
        let (word_pos, bit_pos) = uniswap_v3_math::tick_bitmap::position(tick / tick_spacing);
        let mask = U256::from(1) << bit_pos;

        if let Some(word) = self.tick_bitmap.get_mut(&word_pos) {
            // if the word xor mask flips the bit, then and that of mask will == mask if bit
            // flipped.
            if ((*word ^ mask) & mask) == mask {
                *word ^= mask;
            }
        } else {
            self.tick_bitmap.insert(word_pos, mask);
        }
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

    pub fn calculate_word_pos_bit_pos(compressed: i32) -> (i16, u8) {
        uniswap_v3_math::tick_bitmap::position(compressed)
    }

    #[cfg(test)]
    pub fn set_sqrt_price_x96(&mut self, price: u128) {
        self.sqrt_price_x96 = U256::from(price);
    }

    #[cfg(test)]
    pub fn get_sqrt_price_x96(&self) -> u128 {
        self.sqrt_price_x96.to()
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

impl<I: Iterator<Item = TickData>> TickSpaceFill for I {}

pub trait TickSpaceFill: Iterator<Item = TickData> {
    fn fill_in_missing_ticks(
        mut self,
        zfo: bool,
        tick_spacing: i32
    ) -> impl Iterator<Item = (bool, TickData)>
    where
        Self: Sized + Iterator<Item = TickData>
    {
        let mut result = Vec::new();
        // now that we grabbed start, what we want to do is
        let Some(mut current) = self.next() else { return result.into_iter() };
        result.push((!current.initialized, current));

        for next_tick in self {
            // going down
            let mut diff = if zfo {
                (current.tick - next_tick.tick).as_i32()
            } else {
                (next_tick.tick - current.tick).as_i32()
            };

            // until we hit the tick spacing we want to create new ticks.
            let mut cnt = tick_spacing;
            while diff != tick_spacing {
                let new_tick = TickData {
                    tick:           if zfo {
                        current.tick - I24::unchecked_from(cnt)
                    } else {
                        current.tick + I24::unchecked_from(cnt)
                    },
                    initialized:    false,
                    liquidityGross: 0,
                    liquidityNet:   0
                };
                result.push((false, new_tick));

                cnt += tick_spacing;
                diff -= tick_spacing;
            }
            result.push((!next_tick.initialized, next_tick));
            current = next_tick;
        }

        result.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Once;

    use alloy::primitives::Log;
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
                        .add_directive("angstrom_types_primitives=debug".parse().unwrap())
                        .add_directive("angstrom_types_constants=debug".parse().unwrap())
                        .add_directive("test=debug".parse().unwrap())
                )
                .try_init();
        });
    }

    #[derive(Debug, Clone, Default)]
    struct MockLoader;

    impl PoolDataLoader for MockLoader {
        async fn load_tick_data<P: Provider<N>, N: alloy::network::Network>(
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

        async fn load_pool_data<P: Provider<N>, N: alloy::network::Network>(
            &self,
            _: Option<BlockNumber>,
            _: Arc<P>
        ) -> Result<PoolData, PoolError> {
            unimplemented!()
        }

        fn private_address(&self) -> PoolId {
            unreachable!()
        }

        fn public_address(&self) -> PoolId {
            unreachable!()
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

        fn pool_fee(&self) -> u32 {
            0
        }
    }

    fn setup_basic_pool() -> EnhancedUniswapPool<MockLoader> {
        let mut pool = EnhancedUniswapPool::new(MockLoader, 10);
        pool.token0 = Address::from_slice(&[1u8; 20]);
        pool.token1 = Address::from_slice(&[2u8; 20]);
        pool.token0_decimals = 18;
        pool.token1_decimals = 18;
        pool.liquidity = 1_000_000;
        pool.sqrt_price_x96 = U256::from(1004968906420141727126888u128);
        pool.book_fee = 3000;
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
        let (word_pos, bit_pos) = EnhancedUniswapPool::<DataLoader>::calculate_word_pos_bit_pos(1);
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
        pool.sqrt_price_x96 = sqrt;
        pool.tick = -60;

        let result = pool.fetch_pool_snapshot();
        assert!(result.is_ok(), "{result:?}");

        let (token_a, token_b, snapshot) = result.unwrap();
        assert_eq!(token_a, pool.token0);
        assert_eq!(token_b, pool.token1);
        //assert!(!snapshot.ranges.is_empty());
        println!("Snapshot: {snapshot:#?}");
    }
}
