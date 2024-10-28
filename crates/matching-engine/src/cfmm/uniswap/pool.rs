use std::{collections::HashMap, fmt::Debug, marker::PhantomData, sync::Arc};

use alloy::{
    hex,
    network::Network,
    primitives::{aliases::I24, Address, BlockNumber, B256, I256, U256},
    providers::Provider,
    transports::Transport
};
use alloy_primitives::Log;
use thiserror::Error;
use uniswap_v3_math::{
    error::UniswapV3MathError,
    tick_math::{MAX_SQRT_RATIO, MAX_TICK, MIN_SQRT_RATIO, MIN_TICK}
};

use crate::cfmm::uniswap::{
    i32_to_i24,
    pool_data_loader::{DataLoader, ModifyPositionEvent, PoolDataLoader, TickData},
    ConversionError
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
const MAX_TICKS_PER_REQUEST: u16 = 150;

pub const U256_1: U256 = U256::from_limbs([1, 0, 0, 0]);

#[derive(Debug, Clone, Default)]
pub struct EnhancedUniswapPool<Loader: PoolDataLoader<A> = DataLoader<Address>, A = Address> {
    sync_swap_with_sim:     bool,
    initial_ticks_per_side: u16,
    data_loader:            Loader,
    pub token_a:            Address,
    pub token_a_decimals:   u8,
    pub token_b:            Address,
    pub token_b_decimals:   u8,
    pub liquidity:          u128,
    pub liquidity_net:      i128,
    pub sqrt_price:         U256,
    pub fee:                u32,
    pub tick:               i32,
    pub tick_spacing:       i32,
    pub tick_bitmap:        HashMap<i16, U256>,
    pub ticks:              HashMap<i32, TickInfo>,
    pub _phantom:           PhantomData<A>
}

impl<Loader, A> EnhancedUniswapPool<Loader, A>
where
    Loader: PoolDataLoader<A> + Default,
    A: Debug + Copy + Default
{
    pub fn new(data_loader: Loader, initial_ticks_per_side: u16) -> Self {
        Self {
            initial_ticks_per_side,
            sync_swap_with_sim: false,
            data_loader,
            ..Default::default()
        }
    }

    pub async fn initialize<T: Transport + Clone, N: Network>(
        &mut self,
        block_number: Option<BlockNumber>,
        ws_provider: Arc<impl Provider<T, N>>
    ) -> Result<(), PoolError> {
        self.populate_data(block_number, ws_provider.clone())
            .await?;
        self.sync_ticks(block_number, ws_provider.clone()).await?;
        Ok(())
    }

    pub fn set_sim_swap_sync(&mut self, sync_swap_with_sim: bool) {
        self.sync_swap_with_sim = sync_swap_with_sim;
    }

    pub fn address(&self) -> A {
        self.data_loader.address()
    }

    async fn get_tick_data_batch_request<P: Provider<T, N>, T: Transport + Clone, N: Network>(
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

    async fn sync_ticks<P: Provider<T, N>, T: Transport + Clone, N: Network>(
        &mut self,
        block_number: Option<u64>,
        provider: Arc<P>
    ) -> Result<(), PoolError> {
        if !self.data_is_populated() {
            return Err(PoolError::PoolAlreadyInitialized);
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
                .get_tick_data_batch_request(
                    // safe because we pull the ticks form chain where they are i24
                    i32_to_i24(start_tick)?,
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
                start_tick = last_tick.tick.as_i32();
            } else {
                break;
            }
        }

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
    ) -> Result<SwapResult, SwapSimulationError> {
        if amount_specified.is_zero() {
            return Err(SwapSimulationError::ZeroAmountSpecified);
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
                            .ok_or(SwapSimulationError::LiquidityUnderflow)?
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
            self._sync_from_swap_log(log).map_err(Into::into)
        }
    }

    fn sync_swap_with_sim(&mut self, log: Log) -> Result<(), PoolError> {
        let swap_event = Loader::decode_swap_event(&log)?;

        tracing::trace!(pool_tick = ?self.tick, pool_price = ?self.sqrt_price, pool_liquidity = ?self.liquidity, pool_address = ?self.data_loader.address(), "pool before");
        tracing::debug!(swap_tick=swap_event.tick, swap_price=?swap_event.sqrt_price_x96, swap_liquidity=?swap_event.liquidity, swap_amount0=?swap_event.amount0, swap_amount1=?swap_event.amount1, "swap event");

        let combinations = [
            (self.token_b, swap_event.amount1),
            (self.token_a, swap_event.amount0),
            (self.token_a, swap_event.amount1),
            (self.token_b, swap_event.amount0)
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

        if liquidity_delta != 0 {
            if self.tick > tick_lower && self.tick < tick_upper {
                self.liquidity = if liquidity_delta < 0 {
                    self.liquidity - ((-liquidity_delta) as u128)
                } else {
                    self.liquidity + (liquidity_delta as u128)
                }
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

    pub async fn populate_data<P: Provider<T, N>, T: Transport + Clone, N: Network>(
        &mut self,
        block_number: Option<u64>,
        provider: Arc<P>
    ) -> Result<(), PoolError> {
        let pool_data = self
            .data_loader
            .load_pool_data(block_number, provider)
            .await?;

        self.token_a = pool_data.tokenA;
        self.token_a_decimals = pool_data.tokenADecimals;
        self.token_b = pool_data.tokenB;
        self.token_b_decimals = pool_data.tokenBDecimals;
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
        !(self.token_a.is_zero() || self.token_b.is_zero())
    }

    pub(crate) fn event_signatures(&self) -> Vec<B256> {
        Loader::event_signatures()
    }

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
        let info = self.ticks.entry(tick).or_insert_with(TickInfo::default);

        let liquidity_gross_before = info.liquidity_gross;

        let liquidity_gross_after = if liquidity_delta < 0 {
            liquidity_gross_before - ((-liquidity_delta) as u128)
        } else {
            liquidity_gross_before + (liquidity_delta as u128)
        };

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

        self.tick_bitmap
            .entry(word_pos)
            .and_modify(|word| *word ^= mask)
            .or_insert(mask);
    }

    pub fn get_token_out(&self, token_in: Address) -> Address {
        if self.token_a == token_in {
            self.token_b
        } else {
            self.token_a
        }
    }

    pub fn calculate_word_pos_bit_pos(&self, compressed: i32) -> (i16, u8) {
        uniswap_v3_math::tick_bitmap::position(compressed)
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
    #[error(transparent)]
    SwapSimulationError(#[from] SwapSimulationError),
    #[error(transparent)]
    AlloyContractError(#[from] alloy::contract::Error),
    #[error(transparent)]
    AlloySolTypeError(#[from] alloy::sol_types::Error),
    #[error(transparent)]
    ConversionError(#[from] ConversionError)
}
