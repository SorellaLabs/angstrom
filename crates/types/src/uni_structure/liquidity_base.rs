//! Handles the management of baseline liquidity fetching. This will
//! be the base, in which all other V4 related actions are built ontop of.

use std::collections::HashMap;

use alloy::primitives::{U160, U256};
use itertools::Itertools;
use malachite::num::conversion::traits::SaturatingInto;
use serde::{Deserialize, Serialize};
use uniswap_v3_math::{
    bit_math,
    tick_bitmap::{next_initialized_tick_within_one_word, position},
    tick_math::get_tick_at_sqrt_ratio
};

use crate::matching::{SqrtPriceX96, uniswap::TickInfo};

/// baseline holder for
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineLiquidity {
    pub(super) tick_spacing:     i32,
    pub(super) start_tick:       i32,
    pub(super) start_sqrt_price: SqrtPriceX96,
    pub(super) start_liquidity:  u128,
    /// should only have ticks that are initalized.
    initialized_ticks:           HashMap<i32, TickInfo>,
    /// should only have ticks that are initalized, i.e have liquidity
    tick_bitmap:                 HashMap<i16, U256>
}

impl BaselineLiquidity {
    pub fn new(
        tick_spacing: i32,
        start_tick: i32,
        start_sqrt_price: SqrtPriceX96,
        start_liquidity: u128,
        initialized_ticks: HashMap<i32, TickInfo>,
        tick_bitmap: HashMap<i16, U256>
    ) -> Self {
        Self {
            start_tick,
            start_sqrt_price,
            start_liquidity,
            initialized_ticks,
            tick_bitmap,
            tick_spacing
        }
    }

    /// returns a liquidity ref were the current liquidity is properly
    /// calculated based on were the sqrt_price is at
    pub fn at_sqrt_price<'a>(&'a self, price: SqrtPriceX96) -> eyre::Result<LiquidityAtPoint<'a>> {
        // we are zero for one if the price is going down.
        let zfo = self.start_sqrt_price >= price;
        let tick_at_price = get_tick_at_sqrt_ratio(price.into())?;

        // now that we have the direction, what we need to do is calculate what the
        // current liquidity will be.
        let current_liquidity: i128 = self.start_liquidity.saturating_into();
        // if we are going down
        // let current_tick = self.start_tick;
        let liquidity = if zfo {
            // we want to sort high to low, so that as iterator is consumed, we are going
            // down
            self.initialized_ticks
                .iter()
                .filter(|(t, _)| *t < &self.start_tick)
                // we want high to low.
                .sorted_by_key(|(k, _)| -**k)
                .fold(current_liquidity, |mut acc, (_, info)| {
                    if info.initialized {
                        acc += -info.liquidity_net;
                    }
                    acc
                })
        } else {
            self.initialized_ticks
                .iter()
                .filter(|(t, _)| *t > &self.start_tick)
                // we want low to high
                .sorted_by_key(|(k, _)| *k)
                .fold(current_liquidity, |mut acc, (_, info)| {
                    if info.initialized {
                        acc += info.liquidity_net;
                    }
                    acc
                })
        };

        Ok(LiquidityAtPoint {
            tick_spacing:       self.tick_spacing,
            current_tick:       tick_at_price,
            current_liquidity:  liquidity.unsigned_abs(),
            current_sqrt_price: price,
            initialized_ticks:  &self.initialized_ticks,
            tick_bitmap:        &self.tick_bitmap
        })
    }

    pub fn current<'a>(&'a self) -> LiquidityAtPoint<'a> {
        LiquidityAtPoint {
            tick_spacing:       self.tick_spacing,
            current_tick:       self.start_tick,
            current_sqrt_price: self.start_sqrt_price,
            current_liquidity:  self.start_liquidity,
            initialized_ticks:  &self.initialized_ticks,
            tick_bitmap:        &self.tick_bitmap
        }
    }
}

/// represents the liquidity at a specified point. All operations use this
/// object.

#[derive(Clone, Debug)]
pub struct LiquidityAtPoint<'a> {
    pub(super) tick_spacing:       i32,
    pub(super) current_tick:       i32,
    pub(super) current_sqrt_price: SqrtPriceX96,
    pub(super) current_liquidity:  u128,
    initialized_ticks:             &'a HashMap<i32, TickInfo>,
    tick_bitmap:                   &'a HashMap<i16, U256>
}

impl LiquidityAtPoint<'_> {
    /// moves to the next tick initialized within one word returning
    /// the tick and the liquidity to swap with
    pub fn get_to_next_initialized_tick_within_one_word(
        &self,
        direction: bool
    ) -> eyre::Result<(i32, u128, bool)> {
        let (tick_next, init) = next_initialized_tick_within_one_word(
            self.tick_bitmap,
            self.current_tick,
            self.tick_spacing,
            direction
        )?;

        // adjust self view
        Ok((tick_next, self.current_liquidity, init))
    }

    pub fn get_to_next_initialized_tick_within_one_word_from_tick(
        &self,
        tick: i32,
        direction: bool
    ) -> eyre::Result<(i32, u128, bool)> {
        let (tick_next, init) = next_initialized_tick_within_one_word(
            self.tick_bitmap,
            tick,
            self.tick_spacing,
            direction
        )?;

        // adjust self view
        Ok((tick_next, self.current_liquidity, init))
    }

    pub fn move_to_next_tick(
        &mut self,
        sqrt_price: U256,
        direction: bool,
        full_range: bool,
        sqrt_moved: bool
    ) -> eyre::Result<()> {
        let (tick_next, init) = next_initialized_tick_within_one_word(
            self.tick_bitmap,
            self.current_tick,
            self.tick_spacing,
            direction
        )?;

        if full_range {
            if init {
                let liq_net = self
                    .initialized_ticks
                    .get(&tick_next)
                    .map(|info| if direction { -info.liquidity_net } else { info.liquidity_net })
                    .unwrap_or_default();

                self.current_liquidity = if liq_net < 0 {
                    self.current_liquidity
                        .checked_sub(liq_net.unsigned_abs())
                        .ok_or_else(|| {
                            eyre::eyre!("underflow on liquidity. Shouldn't be possible")
                        })?
                } else {
                    self.current_liquidity + (liq_net.unsigned_abs())
                };
            }
            if direction {
                self.current_tick = tick_next - 1
            } else {
                self.current_tick = tick_next
            };
        } else if sqrt_moved {
            self.current_tick = get_tick_at_sqrt_ratio(sqrt_price)?;
        }

        Ok(())
    }

    pub fn set_sqrt_price(&mut self, sqrt_price: U256) {
        self.current_sqrt_price = sqrt_price.into();
    }

    fn get_next_tick_gt(&mut self) -> (i32, bool) {
        let (word_pos, bit) = position((self.current_tick / self.tick_spacing) + 1);
        let word = self.tick_bitmap.get(&word_pos).cloned().unwrap_or_default();

        // least sig bytes
        let rel_pos = bit_math::least_significant_bit(word >> bit)
            .map(|v| U256::from(v))
            .unwrap_or_else(|_| U256::from(256));

        let initialized = rel_pos != U256::from(256);
        let next_bit_pos = if initialized { rel_pos.to::<u8>() + bit } else { u8::MAX };
        let tick = self.to_tick(word_pos, next_bit_pos);
        self.current_tick = tick;

        (tick, initialized)
    }

    fn get_next_tick_lt(&mut self) -> (i32, bool) {
        let (word_pos, bit) = position((self.current_tick / self.tick_spacing) - 1);
        let word = self.tick_bitmap.get(&word_pos).cloned().unwrap_or_default();
        let offset = 255 - bit;

        let rel_pos = bit_math::most_significant_bit(word << offset)
            .map(|v| U256::from(v))
            .unwrap_or_else(|_| U256::from(256));

        let initialized = rel_pos != U256::from(256);
        let next_bit_pos = if initialized { rel_pos.to::<u8>() - offset } else { 0 };
        let tick = self.to_tick(word_pos, next_bit_pos);
        self.current_tick = tick;

        (tick, initialized)
    }

    fn to_tick(&self, word: i16, bit: u8) -> i32 {
        ((word as i32) * 256 + bit as i32) * self.tick_spacing
    }

    pub fn generate_checksum_to(self, end_tick: i32) -> eyre::Result<U160> {
        let direction = self.current_tick > end_tick;

        // reward up
        if direction {
            // let start_tick =
        }
        // reward down
        else {
        }

        todo!()
    }
}
