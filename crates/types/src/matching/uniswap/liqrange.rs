use std::{fmt::Debug, ops::Deref};

use eyre::eyre;
use serde::{Deserialize, Serialize};
use uniswap_v3_math::tick_math::{MAX_TICK, MIN_TICK};

use super::{Direction, PoolPrice, PoolSnapshot, Tick};
use crate::matching::SqrtPriceX96;

/// A LiqRange describes the liquidity conditions within a specific range of
/// ticks.  A LiqRange covers ticks [lower_tick, upper_tick)
#[derive(Default, Debug, Clone, PartialEq, Eq, Copy, Serialize, Deserialize)]
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

#[derive(Copy, Clone)]
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

    /// Determines if a given SqrtPriceX96 is within this liquidity range
    pub fn price_in_range(&self, price: SqrtPriceX96) -> bool {
        if let Ok(price_tick) = price.to_tick() {
            price_tick >= self.lower_tick && price_tick < self.upper_tick
        } else {
            false
        }
    }

    pub fn start_tick(&self, direction: Direction) -> Tick {
        match direction {
            Direction::BuyingT0 => self.lower_tick,
            Direction::SellingT0 => self.upper_tick
        }
    }

    /// Returns the final tick in this liquidity range presuming the price
    /// starts
    pub fn end_tick(&self, direction: Direction) -> Tick {
        match direction {
            Direction::BuyingT0 => self.upper_tick,
            Direction::SellingT0 => self.lower_tick
        }
    }

    /// PoolPrice representing the start price of this liquidity bound
    pub fn start_price(&self, direction: Direction) -> PoolPrice<'a> {
        let tick = self.start_tick(direction);
        PoolPrice { tick, liq_range: *self, price: SqrtPriceX96::at_tick(tick).unwrap() }
    }

    /// PoolPrice representing the end price of this liquidity bound
    pub fn end_price(&self, direction: Direction) -> PoolPrice<'a> {
        let tick = self.end_tick(direction);
        PoolPrice { tick, liq_range: *self, price: SqrtPriceX96::at_tick(tick).unwrap() }
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

impl<'a> Debug for LiqRangeRef<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut builder = f.debug_struct("LiqRangeRef");
        builder.field("range", &self.range);
        builder.field("range_idx", &self.range_idx);
        builder.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matching::uniswap::Direction;

    #[test]
    fn test_liq_range_creation() {
        // Test valid range creation
        let result = LiqRange::new(-1000, 1000, 100);
        assert!(result.is_ok());
        let range = result.unwrap();
        assert_eq!(range.lower_tick(), -1000);
        assert_eq!(range.upper_tick(), 1000);
        assert_eq!(range.liquidity(), 100);

        // Test invalid ranges
        assert!(LiqRange::new(1000, -1000, 100).is_err()); // Upper < Lower
        assert!(LiqRange::new(-887273, 1000, 100).is_err()); // Below MIN_TICK
        assert!(LiqRange::new(-1000, 887273, 100).is_err()); // Above MAX_TICK
        assert!(LiqRange::new(100, 100, 100).is_err()); // Equal bounds
    }

    #[test]
    fn test_liq_range_getters() {
        let range = LiqRange::new(-100, 100, 1000).unwrap();
        assert_eq!(range.lower_tick(), -100);
        assert_eq!(range.upper_tick(), 100);
        assert_eq!(range.liquidity(), 1000);
    }

    #[test]
    fn test_liq_range_ref_price_in_range() {
        let pool_snap = PoolSnapshot::default();
        let range = LiqRange::new(-100, 100, 1000).unwrap();
        let range_ref = LiqRangeRef::new(&pool_snap, &range, 0);

        // Test prices within range
        let price_in = SqrtPriceX96::at_tick(0).unwrap();
        assert!(range_ref.price_in_range(price_in));

        // Test prices outside range
        let price_below = SqrtPriceX96::at_tick(-150).unwrap();
        let price_above = SqrtPriceX96::at_tick(150).unwrap();
        assert!(!range_ref.price_in_range(price_below));
        assert!(!range_ref.price_in_range(price_above));
    }

    #[test]
    fn test_liq_range_ref_direction_ticks() {
        let pool_snap = PoolSnapshot::default();
        let range = LiqRange::new(-100, 100, 1000).unwrap();
        let range_ref = LiqRangeRef::new(&pool_snap, &range, 0);

        // Test buying direction
        assert_eq!(range_ref.start_tick(Direction::BuyingT0), -100);
        assert_eq!(range_ref.end_tick(Direction::BuyingT0), 100);

        // Test selling direction
        assert_eq!(range_ref.start_tick(Direction::SellingT0), 100);
        assert_eq!(range_ref.end_tick(Direction::SellingT0), -100);
    }

    #[test]
    fn test_liq_range_ref_prices() {
        let pool_snap = PoolSnapshot::default();
        let range = LiqRange::new(-100, 100, 1000).unwrap();
        let range_ref = LiqRangeRef::new(&pool_snap, &range, 0);

        // Test start prices
        let start_price_buying = range_ref.start_price(Direction::BuyingT0);
        let start_price_selling = range_ref.start_price(Direction::SellingT0);
        assert_eq!(start_price_buying.tick, -100);
        assert_eq!(start_price_selling.tick, 100);

        // Test end prices
        let end_price_buying = range_ref.end_price(Direction::BuyingT0);
        let end_price_selling = range_ref.end_price(Direction::SellingT0);
        assert_eq!(end_price_buying.tick, 100);
        assert_eq!(end_price_selling.tick, -100);
    }

    #[test]
    fn test_liq_range_ref_donate_tick() {
        let pool_snap = PoolSnapshot::default();
        let range = LiqRange::new(-100, 100, 1000).unwrap();
        let range_ref = LiqRangeRef::new(&pool_snap, &range, 0);

        assert_eq!(range_ref.donate_tick(), -100);
    }

    #[test]
    fn test_liq_range_ref_equality() {
        let pool_snap = PoolSnapshot::default();
        let range = LiqRange::new(-100, 100, 1000).unwrap();

        let ref1 = LiqRangeRef::new(&pool_snap, &range, 0);
        let ref2 = LiqRangeRef::new(&pool_snap, &range, 0);
        let ref3 = LiqRangeRef::new(&pool_snap, &range, 1);

        assert_eq!(ref1, ref2);
        assert_ne!(ref1, ref3);
    }

    #[test]
    fn test_liq_range_ref_debug() {
        let pool_snap = PoolSnapshot::default();
        let range = LiqRange::new(-100, 100, 1000).unwrap();
        let range_ref = LiqRangeRef::new(&pool_snap, &range, 0);

        let debug_output = format!("{:?}", range_ref);
        assert!(debug_output.contains("LiqRangeRef"));
        assert!(debug_output.contains("range_idx: 0"));
    }

    #[test]
    fn test_liq_range_ref_deref() {
        let pool_snap = PoolSnapshot::default();
        let range = LiqRange::new(-100, 100, 1000).unwrap();
        let range_ref = LiqRangeRef::new(&pool_snap, &range, 0);

        // Test deref functionality
        assert_eq!(range_ref.lower_tick(), -100);
        assert_eq!(range_ref.upper_tick(), 100);
        assert_eq!(range_ref.liquidity(), 1000);
    }
}
