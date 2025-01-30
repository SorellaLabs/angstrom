use std::{fmt::Debug, slice::Iter};

use eyre::{eyre, Context, OptionExt};
use serde::{Deserialize, Serialize};
use uniswap_v3_math::tick_math::get_tick_at_sqrt_ratio;

use super::{
    liqrange::{LiqRange, LiqRangeRef},
    poolprice::PoolPrice,
    Tick
};
use crate::matching::{math::low_to_high, SqrtPriceX96};

/// Snapshot of a particular Uniswap pool and a map of its liquidity.
#[derive(Default, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolSnapshot {
    /// Known tick ranges and liquidity positions gleaned from the market
    /// snapshot
    pub ranges:                Vec<LiqRange>,
    /// The current SqrtPriceX96 for this pairing as of this snapshot
    /// (𝛥Token1/𝛥Token0)
    pub(crate) sqrt_price_x96: SqrtPriceX96,
    /// The current tick our price lives in - price might not be precisely on a
    /// tick bound, this is the LOWER of the possible ticks
    pub(crate) current_tick:   Tick,
    /// Index into the 'ranges' vector for the PoolRange that includes the tick
    /// our current price lives at/in
    pub(crate) cur_tick_idx:   usize
}

impl PoolSnapshot {
    pub fn new(mut ranges: Vec<LiqRange>, sqrt_price_x96: SqrtPriceX96) -> eyre::Result<Self> {
        // Sort our ranges
        ranges.sort_by(|a, b| a.lower_tick.cmp(&b.lower_tick));

        // Ensure the ranges are contiguous
        if !ranges
            .windows(2)
            .all(|w| w[0].upper_tick == w[1].lower_tick)
        {
            return Err(eyre!("Tick windows not contiguous, cannot create snapshot"));
        }

        // Get our current tick from our current price
        let current_tick = get_tick_at_sqrt_ratio(sqrt_price_x96.into()).wrap_err_with(|| {
            eyre!("Unable to get a tick from our current price '{:?}'", sqrt_price_x96)
        })?;

        // Find the tick range that our current tick lies within
        let Some(cur_tick_idx) = ranges
            .iter()
            .position(|r| r.lower_tick <= current_tick && current_tick < r.upper_tick)
        else {
            return Err(eyre!(
                "Unable to find initialized tick window for tick '{}'\n {:?}",
                current_tick,
                ranges
            ));
        };

        Ok(Self { ranges, sqrt_price_x96, current_tick, cur_tick_idx })
    }

    /// Find the PoolRange in this market snapshot that the provided tick lies
    /// within, if any
    pub fn get_range_for_tick(&self, tick: Tick) -> Option<LiqRangeRef> {
        self.ranges
            .iter()
            .enumerate()
            .find(|(_, r)| r.lower_tick <= tick && tick < r.upper_tick)
            .map(|(range_idx, range)| LiqRangeRef { pool_snap: self, range, range_idx })
    }

    /// Returns a list of references to all liquidity ranges including and
    /// between the given Ticks.  These ranges will be continuous in order.
    pub fn ranges_for_ticks(
        &self,
        start_tick: Tick,
        end_tick: Tick
    ) -> eyre::Result<Vec<LiqRangeRef>> {
        let (low, high) = low_to_high(&start_tick, &end_tick);
        let output = self
            .ranges
            .iter()
            .enumerate()
            .filter_map(|(range_idx, range)| {
                if range.upper_tick > *low && range.lower_tick <= *high {
                    Some(LiqRangeRef { pool_snap: self, range, range_idx })
                } else {
                    None
                }
            })
            .collect();
        Ok(output)
    }

    /// Return a read-only iterator over the liquidity ranges in this snapshot
    pub fn ranges(&self) -> Iter<LiqRange> {
        self.ranges.iter()
    }

    pub fn current_price(&self) -> PoolPrice {
        let range = self
            .ranges
            .get(self.cur_tick_idx)
            .map(|range| LiqRangeRef { pool_snap: self, range, range_idx: self.cur_tick_idx })
            .unwrap();
        PoolPrice { liq_range: range, tick: self.current_tick, price: self.sqrt_price_x96 }
    }

    pub fn at_price(&self, price: SqrtPriceX96) -> eyre::Result<PoolPrice> {
        let tick = price.to_tick()?;
        let range = self
            .get_range_for_tick(tick)
            .ok_or_eyre("Unable to find tick range for price")?;
        Ok(PoolPrice { liq_range: range, tick, price })
    }

    pub fn at_tick(&self, tick: i32) -> eyre::Result<PoolPrice> {
        let price = SqrtPriceX96::at_tick(tick)?;
        let range = self
            .get_range_for_tick(tick)
            .ok_or_eyre("Unable to find tick range for price")?;
        Ok(PoolPrice { liq_range: range, tick, price })
    }

    pub fn liquidity_at_tick(&self, tick: Tick) -> Option<u128> {
        self.get_range_for_tick(tick).map(|range| range.liquidity())
    }
}

#[cfg(test)]
mod tests {
    use eyre::Result;

    use super::*;

    fn create_basic_ranges() -> Vec<LiqRange> {
        vec![
            LiqRange { liquidity: 1_000_000_u128, lower_tick: 100, upper_tick: 200 },
            LiqRange { liquidity: 2_000_000_u128, lower_tick: 200, upper_tick: 300 },
            LiqRange { liquidity: 3_000_000_u128, lower_tick: 300, upper_tick: 400 },
        ]
    }

    #[test]
    fn test_new_pool_snapshot() -> Result<()> {
        let ranges = create_basic_ranges();
        let price = SqrtPriceX96::at_tick(250)?;

        let snapshot = PoolSnapshot::new(ranges.clone(), price)?;

        assert_eq!(snapshot.ranges, ranges);
        assert_eq!(snapshot.sqrt_price_x96, price);
        assert_eq!(snapshot.current_tick, 250);
        assert_eq!(snapshot.cur_tick_idx, 1); // Should be in the second range

        Ok(())
    }

    #[test]
    fn test_non_contiguous_ranges() {
        let ranges = vec![
            LiqRange { liquidity: 1_000_000_u128, lower_tick: 100, upper_tick: 200 },
            LiqRange {
                liquidity:  2_000_000_u128,
                lower_tick: 300, // Gap between 200 and 300!
                upper_tick: 400
            },
        ];
        let price = SqrtPriceX96::at_tick(150).unwrap();

        let result = PoolSnapshot::new(ranges, price);
        assert!(result.is_err(), "Should fail with non-contiguous ranges");
    }

    #[test]
    fn test_get_range_for_tick() -> Result<()> {
        let ranges = create_basic_ranges();
        let price = SqrtPriceX96::at_tick(250)?;
        let snapshot = PoolSnapshot::new(ranges, price)?;

        // Test tick in middle range
        let range = snapshot.get_range_for_tick(250);
        assert!(range.is_some());
        assert_eq!(range.unwrap().range.liquidity, 2_000_000_u128);

        // Test tick at range boundary
        let range = snapshot.get_range_for_tick(200);
        assert!(range.is_some());
        assert_eq!(range.unwrap().range.liquidity, 2_000_000_u128);

        // Test tick outside ranges
        let range = snapshot.get_range_for_tick(50);
        assert!(range.is_none());

        Ok(())
    }

    #[test]
    fn test_ranges_for_ticks() -> Result<()> {
        let ranges = create_basic_ranges();
        let price = SqrtPriceX96::at_tick(250)?;
        let snapshot = PoolSnapshot::new(ranges, price)?;

        // Test getting multiple ranges
        let ranges = snapshot.ranges_for_ticks(150, 350)?;
        assert_eq!(ranges.len(), 3);
        assert_eq!(ranges[0].range.liquidity, 1_000_000_u128);
        assert_eq!(ranges[1].range.liquidity, 2_000_000_u128);
        assert_eq!(ranges[2].range.liquidity, 3_000_000_u128);

        // Test single range
        let ranges = snapshot.ranges_for_ticks(220, 280)?;
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].range.liquidity, 2_000_000_u128);

        // Test reversed ticks (should still work)
        let ranges = snapshot.ranges_for_ticks(350, 150)?;
        assert_eq!(ranges.len(), 3);

        Ok(())
    }

    #[test]
    fn test_current_price() -> Result<()> {
        let ranges = create_basic_ranges();
        let price = SqrtPriceX96::at_tick(250)?;
        let snapshot = PoolSnapshot::new(ranges, price)?;

        let current_price = snapshot.current_price();
        assert_eq!(current_price.price, price);
        assert_eq!(current_price.tick, 250);
        assert_eq!(current_price.liq_range.range.liquidity, 2_000_000_u128);

        Ok(())
    }

    #[test]
    fn test_at_price() -> Result<()> {
        let ranges = create_basic_ranges();
        let init_price = SqrtPriceX96::at_tick(250)?;
        let snapshot = PoolSnapshot::new(ranges, init_price)?;

        let new_price = SqrtPriceX96::at_tick(350)?;
        let price_point = snapshot.at_price(new_price)?;

        assert_eq!(price_point.price, new_price);
        assert_eq!(price_point.tick, 350);
        assert_eq!(price_point.liq_range.range.liquidity, 3_000_000_u128);

        // Test invalid price
        let invalid_price = SqrtPriceX96::at_tick(50)?;
        assert!(snapshot.at_price(invalid_price).is_err());

        Ok(())
    }

    #[test]
    fn test_at_tick() -> Result<()> {
        let ranges = create_basic_ranges();
        let price = SqrtPriceX96::at_tick(250)?;
        let snapshot = PoolSnapshot::new(ranges, price)?;

        let price_point = snapshot.at_tick(350)?;
        assert_eq!(price_point.tick, 350);
        assert_eq!(price_point.liq_range.range.liquidity, 3_000_000_u128);

        // Test invalid tick
        assert!(snapshot.at_tick(50).is_err());

        Ok(())
    }

    #[test]
    fn test_liquidity_at_tick() -> Result<()> {
        let ranges = create_basic_ranges();
        let price = SqrtPriceX96::at_tick(250)?;
        let snapshot = PoolSnapshot::new(ranges, price)?;

        assert_eq!(snapshot.liquidity_at_tick(150), Some(1_000_000_u128));
        assert_eq!(snapshot.liquidity_at_tick(250), Some(2_000_000_u128));
        assert_eq!(snapshot.liquidity_at_tick(350), Some(3_000_000_u128));
        assert_eq!(snapshot.liquidity_at_tick(50), None);

        Ok(())
    }
}
