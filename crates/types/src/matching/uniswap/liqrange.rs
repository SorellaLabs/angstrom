use std::ops::Deref;

use eyre::eyre;
use uniswap_v3_math::tick_math::{MAX_TICK, MIN_TICK};

use super::{Direction, PoolSnapshot, Tick};

/// A LiqRange describes the liquidity conditions within a specific range of
/// ticks.
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct LiqRange {
    /// Lower tick for this range
    pub(super) lower_tick: Tick,
    /// Upper tick for this range
    pub(super) upper_tick: Tick,
    /// Total liquidity within this range
    pub(super) liquidity:  u128
}

impl LiqRange {
    pub fn new(lower_tick: Tick, upper_tick: Tick, liquidity: u128) -> eyre::Result<Self> {
        // Validate our inputs
        if upper_tick <= lower_tick {
            return Err(eyre!(
                "Upper tick bound less than or equal to lower tick bound for range ({}, {})",
                lower_tick,
                upper_tick
            ));
        }
        if upper_tick > MAX_TICK {
            return Err(eyre!("Proposed upper tick '{}' out of valid tick range", upper_tick));
        }
        if lower_tick < MIN_TICK {
            return Err(eyre!("Proposed lower tick '{}' out of valid tick range", lower_tick));
        }
        Ok(Self { lower_tick, upper_tick, liquidity })
    }

    pub fn lower_tick(&self) -> i32 {
        self.lower_tick
    }

    pub fn upper_tick(&self) -> i32 {
        self.upper_tick
    }

    pub fn liquidity(&self) -> u128 {
        self.liquidity
    }
}

#[derive(Copy, Clone, Debug)]
pub struct LiqRangeRef<'a> {
    pub(super) pool_snap: &'a PoolSnapshot,
    pub(super) range:     &'a LiqRange,
    pub(super) range_idx: usize
}

impl<'a> Deref for LiqRangeRef<'a> {
    type Target = LiqRange;

    fn deref(&self) -> &Self::Target {
        self.range
    }
}

impl<'a> PartialEq for LiqRangeRef<'a> {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.pool_snap, other.pool_snap)
            && std::ptr::eq(self.range, other.range)
            && self.range_idx == other.range_idx
    }
}

impl<'a> Eq for LiqRangeRef<'a> {}

impl<'a> LiqRangeRef<'a> {
    pub fn new(market: &'a PoolSnapshot, range: &'a LiqRange, range_idx: usize) -> Self {
        Self { pool_snap: market, range, range_idx }
    }

    /// Returns the final tick in this liquidity range presuming the price
    /// starts
    pub fn end_bound(&self, direction: Direction) -> Tick {
        match direction {
            Direction::BuyingT0 => self.upper_tick,
            Direction::SellingT0 => self.lower_tick
        }
    }

    /// Returns the appropriate tick to donate to in order to reward LPs in this
    /// position
    pub fn donate_tick(&self) -> Tick {
        self.lower_tick
    }

    pub fn next(&self, direction: Direction) -> Option<Self> {
        match direction {
            Direction::BuyingT0 => self.pool_snap.get_range_for_tick(self.range.upper_tick),
            Direction::SellingT0 => self.pool_snap.get_range_for_tick(self.range.lower_tick - 1)
        }
    }
}
