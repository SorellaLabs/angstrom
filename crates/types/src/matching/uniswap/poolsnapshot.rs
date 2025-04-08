use std::{fmt::Debug, slice::Iter};

use alloy_primitives::{U160, U256, aliases::I24, utils::keccak256};
use eyre::{Context, OptionExt, eyre};
use serde::{Deserialize, Serialize};
use uniswap_v3_math::tick_math::get_tick_at_sqrt_ratio;

use super::{
    Tick,
    liqrange::{LiqRange, LiqRangeRef},
    poolprice::PoolPrice
};
use crate::{
    matching::{SqrtPriceX96, math::low_to_high},
    sol_bindings::Ray
};

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
    pub(crate) cur_tick_idx:   usize,
    /// Tick spacing of our pool
    pub(crate) tick_spacing:   i32
}

impl PoolSnapshot {
    pub fn new(
        tick_spacing: i32,
        mut ranges: Vec<LiqRange>,
        sqrt_price_x96: SqrtPriceX96
    ) -> eyre::Result<Self> {
        // Sort our ranges
        ranges.sort_by(|a, b| a.lower_tick.cmp(&b.lower_tick));

        // Tick spacing must be a positive integer
        if tick_spacing <= 0 {
            return Err(eyre!("Invalid tick spacing: {tick_spacing}"));
        }

        // If we had only a single range, then `windows` wouldn't have iterated at all
        // so let's check that quick
        if ranges.len() == 1 {
            for t in [ranges[0].lower_tick, ranges[0].upper_tick] {
                if t % tick_spacing != 0 {
                    return Err(eyre!(
                        "Provided tick '{t}' not aligned with tick spacing {tick_spacing}"
                    ));
                }
            }
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

        Ok(Self { ranges, sqrt_price_x96, current_tick, cur_tick_idx, tick_spacing })
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
    /// between the given Ticks.  These ranges will be continuous in order, and
    /// in the order specified from the range for start_tick to the range for
    /// end_tick
    pub fn ranges_for_ticks(
        &self,
        start_tick: Tick,
        end_tick: Tick
    ) -> eyre::Result<Vec<LiqRangeRef>> {
        let (low, high) = low_to_high(&start_tick, &end_tick);
        let mut output = self
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
            .collect::<Vec<_>>();

        // If we're going high to low, reverse our solution
        if start_tick > end_tick {
            output.reverse();
        }

        Ok(output)
    }

    /// Returns a vec of LiqRangeRef describing the liquidity ranges from the
    /// given start tick to the snapshot's current tick.  The ranges returned
    /// will be ordered so that they start at the range associated with the
    /// start tick and end at the range of the snapshot's current tick
    pub fn ranges_from_tick(&self, start_tick: i32) -> eyre::Result<Vec<LiqRangeRef>> {
        self.ranges_for_ticks(start_tick, self.current_tick)
    }

    pub fn checksum_from_ticks(&self, start_tick: Tick, end_tick: Tick) -> eyre::Result<U160> {
        let from_above = start_tick > end_tick;
        let (ticks, liquidity): (Vec<_>, Vec<_>) = self
            .ranges_for_ticks(start_tick, end_tick)?
            .iter()
            .filter(|tick| tick.is_initialized)
            .map(|r| {
                let target_tick = if from_above { r.lower_tick } else { r.upper_tick };
                (target_tick, r.liquidity)
            })
            .unzip();
        // We want to skip the last tick (representing the current range) but skip the
        // first liquidity (representing start_liquidity)
        let checksum_bytes = ticks
            .iter()
            .take(ticks.len() - 1)
            .zip(liquidity.iter().skip(1))
            .fold([0u8; 32], |acc, (tick, liquidity)| {
                let tick_bytes: [u8; 3] = I24::unchecked_from(*tick).to_be_bytes();
                let hash_input = [acc.as_slice(), &liquidity.to_be_bytes(), &tick_bytes].concat();
                *keccak256(&hash_input)
            });
        Ok(U160::from(U256::from_be_bytes(checksum_bytes) >> 96))
    }

    pub fn checksum_from(&self, bound_tick: Tick) -> eyre::Result<U160> {
        self.checksum_from_ticks(bound_tick, self.current_tick)
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

    pub fn is_bid(&self, price: Ray) -> bool {
        let end_price = SqrtPriceX96::from(price);
        let start_price = self.sqrt_price_x96;

        start_price < end_price
    }
}

#[cfg(test)]
mod tests {
    use uniswap_v3_math::sqrt_price_math::{_get_amount_0_delta, _get_amount_1_delta};

    use super::*;

    impl PoolSnapshot {
        fn get_deltas(&self, start_price: SqrtPriceX96, end_price: SqrtPriceX96) -> Option<u128> {
            let is_bid = start_price < end_price;
            // fetch ticks for ranges
            let start_tick = get_tick_at_sqrt_ratio(start_price.into()).ok()?;
            let end_tick = get_tick_at_sqrt_ratio(end_price.into()).ok()?;

            // Get all affected liquidity ranges
            let liq_range = self.ranges_for_ticks(start_tick, end_tick).ok()?;
            let mut t_delta = 0u128;

            for range in &liq_range {
                let (range_start_price, range_end_price) = if is_bid {
                    let start = start_price.max(SqrtPriceX96::at_tick(range.lower_tick).ok()?);
                    let end = end_price.min(SqrtPriceX96::at_tick(range.upper_tick).ok()?);
                    (start, end)
                } else {
                    let start = start_price.min(SqrtPriceX96::at_tick(range.upper_tick).ok()?);
                    let end = end_price.max(SqrtPriceX96::at_tick(range.lower_tick).ok()?);
                    (start, end)
                };

                // Skip if the range is not relevant
                if (is_bid && range_start_price >= range_end_price)
                    || (!is_bid && range_start_price <= range_end_price)
                {
                    continue;
                }

                let delta = if is_bid {
                    _get_amount_0_delta(
                        range_start_price.into(),
                        range_end_price.into(),
                        range.liquidity,
                        true
                    )
                    .ok()?
                    .to::<u128>()
                } else {
                    _get_amount_1_delta(
                        range_end_price.into(),
                        range_start_price.into(),
                        range.liquidity,
                        true
                    )
                    .ok()?
                    .to::<u128>()
                };
                t_delta += delta

                //
                // total_in += amount_in.to::<u128>() + fee.to::<u128>();
                // total_out += amount_out.to::<u128>();

                // total_delta += delta;
            }

            Some(t_delta)
        }
    }

    fn setup_basic_pool() -> PoolSnapshot {
        // Create a simple pool with three tick ranges
        let ranges = vec![
            LiqRange {
                lower_tick:     0,
                upper_tick:     100,
                liquidity:      1000,
                is_tick_edge:   false,
                is_initialized: true
            },
            LiqRange {
                lower_tick:     100,
                upper_tick:     200,
                liquidity:      2000,
                is_tick_edge:   false,
                is_initialized: true
            },
            LiqRange {
                lower_tick:     200,
                upper_tick:     300,
                liquidity:      1500,
                is_tick_edge:   false,
                is_initialized: true
            },
        ];

        // Start price in the middle range (tick 150)
        let sqrt_price_x96 = SqrtPriceX96::at_tick(150).unwrap();
        PoolSnapshot::new(10, ranges, sqrt_price_x96).unwrap()
    }

    #[test]
    fn test_get_deltas_bid_direction() {
        let pool = setup_basic_pool();

        // Test moving from tick 150 to tick 250 (upward/bid movement)
        let start_price = SqrtPriceX96::at_tick(150).unwrap();
        let end_price = SqrtPriceX96::at_tick(250).unwrap();

        let delta = pool.get_deltas(start_price, end_price).unwrap();

        // We should get a non-zero amount
        assert!(delta > 0, "Delta should be positive for bid direction");

        // Test the reverse direction to ensure it's different
        let reverse_delta = pool.get_deltas(end_price, start_price).unwrap();
        assert_ne!(delta, reverse_delta, "Bid and ask deltas should differ");
    }

    #[test]
    fn test_get_deltas_within_same_range() {
        let pool = setup_basic_pool();

        // Test movement within the same tick range (150 to 180)
        let start_price = SqrtPriceX96::at_tick(150).unwrap();
        let end_price = SqrtPriceX96::at_tick(180).unwrap();

        let delta = pool.get_deltas(start_price, end_price).unwrap();
        assert!(delta > 0, "Delta should be positive within same range");

        // Test a smaller price movement
        let small_end_price = SqrtPriceX96::at_tick(160).unwrap();
        let small_delta = pool.get_deltas(start_price, small_end_price).unwrap();
        assert!(small_delta < delta, "Smaller price movement should result in smaller delta");
    }

    #[test]
    fn test_get_deltas_cross_multiple_ranges() {
        let pool = setup_basic_pool();

        // Test movement across multiple ranges (50 to 250)
        let start_price = SqrtPriceX96::at_tick(50).unwrap();
        let end_price = SqrtPriceX96::at_tick(250).unwrap();

        let delta = pool.get_deltas(start_price, end_price).unwrap();

        // Test movement within a single range
        let single_range_start = SqrtPriceX96::at_tick(120).unwrap();
        let single_range_end = SqrtPriceX96::at_tick(180).unwrap();
        let single_range_delta = pool
            .get_deltas(single_range_start, single_range_end)
            .unwrap();

        assert!(
            delta > single_range_delta,
            "Cross-range delta should be larger than single range delta"
        );
    }

    #[test]
    fn test_get_deltas_edge_cases() {
        let pool = setup_basic_pool();

        // Test movement at range boundaries
        let start_price = SqrtPriceX96::at_tick(100).unwrap(); // Exactly at a range boundary
        let end_price = SqrtPriceX96::at_tick(200).unwrap(); // Another range boundary

        let delta = pool.get_deltas(start_price, end_price);
        assert!(delta.is_some(), "Should handle range boundary movements");

        // Test minimal movement
        let tiny_move_end = SqrtPriceX96::at_tick(151).unwrap();
        let tiny_delta = pool.get_deltas(start_price, tiny_move_end).unwrap();
        assert!(tiny_delta > 0, "Should handle minimal price movements");
    }

    #[test]
    fn test_get_deltas_liquidity_impact() {
        // Create two pools with different liquidity profiles
        let high_liq_ranges = vec![
            LiqRange {
                lower_tick:     0,
                upper_tick:     100,
                liquidity:      10000, // 10x more liquidity
                is_tick_edge:   false,
                is_initialized: true
            },
            LiqRange {
                lower_tick:     100,
                upper_tick:     200,
                liquidity:      20000,
                is_tick_edge:   false,
                is_initialized: true
            },
        ];

        let low_liq_ranges = vec![
            LiqRange {
                lower_tick:     0,
                upper_tick:     100,
                liquidity:      1000,
                is_tick_edge:   false,
                is_initialized: true
            },
            LiqRange {
                lower_tick:     100,
                upper_tick:     200,
                liquidity:      2000,
                is_tick_edge:   false,
                is_initialized: true
            },
        ];

        let sqrt_price_x96 = SqrtPriceX96::at_tick(50).unwrap();
        let high_liq_pool = PoolSnapshot::new(10, high_liq_ranges, sqrt_price_x96).unwrap();
        let low_liq_pool = PoolSnapshot::new(10, low_liq_ranges, sqrt_price_x96).unwrap();

        let start_price = SqrtPriceX96::at_tick(50).unwrap();
        let end_price = SqrtPriceX96::at_tick(150).unwrap();

        let high_liq_delta = high_liq_pool.get_deltas(start_price, end_price).unwrap();
        let low_liq_delta = low_liq_pool.get_deltas(start_price, end_price).unwrap();

        assert!(high_liq_delta > low_liq_delta, "Higher liquidity should result in larger delta");
    }
}
