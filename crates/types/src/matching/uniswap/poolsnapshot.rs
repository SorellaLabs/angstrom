use std::slice::Iter;

use eyre::{eyre, Context, OptionExt};
use uniswap_v3_math::tick_math::get_tick_at_sqrt_ratio;

use super::{
    liqrange::{LiqRange, LiqRangeRef},
    poolprice::PoolPrice,
    Tick
};
use crate::matching::SqrtPriceX96;

/// Snapshot of a particular Uniswap pool and a map of its liquidity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PoolSnapshot {
    /// Known tick ranges and liquidity positions gleaned from the market
    /// snapshot
    pub(crate) ranges:         Vec<LiqRange>,
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
            return Err(eyre!("Unable to find initialized tick window for tick '{}'", current_tick));
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

    pub fn liquidity_at_tick(&self, tick: Tick) -> Option<u128> {
        self.get_range_for_tick(tick).map(|range| range.liquidity())
    }
}
