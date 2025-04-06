use std::collections::HashMap;

use eyre::eyre;

use super::{PoolPriceVec, Tick};
use crate::matching::{SqrtPriceX96, math::low_to_high};

#[derive(Debug)]
pub struct DonationResult {
    /// the donation for the current tick.
    pub current_tick:   u128,
    /// HashMap from liquidity range bounds `(lower_tick, upper_tick)` to
    /// donation quantity in T0
    pub tick_donations: HashMap<(Tick, Tick), (bool, u128)>,
    pub final_price:    SqrtPriceX96,
    /// Total amount of donation in T0
    pub total_donated:  u128
}

impl DonationResult {
    pub fn new(
        current_tick: u128,
        tick_donations: HashMap<(Tick, Tick), (bool, u128)>,
        final_price: SqrtPriceX96,
        total_donated: u128
    ) -> Self {
        Self { current_tick, tick_donations, final_price, total_donated }
    }

    /// Combine this donation result with another donation result.  Donations to
    /// the same liquidity segment will be summed.  This method will also check
    /// to ensure that our output is a set of donations to a contiguous range.
    pub fn combine(&self, rhs: &Self) -> eyre::Result<Self> {
        // Merge our two HashMaps
        let mut tick_donations = HashMap::new();
        for (k, v) in self.tick_donations.iter().chain(rhs.tick_donations.iter()) {
            tick_donations
                .entry(*k)
                .and_modify(|(_, e)| *e += v.1)
                .or_insert(*v);
        }

        // Validate that we're still covering a contiguous range
        let mut all_ranges = tick_donations.keys().collect::<Vec<_>>();
        all_ranges.sort_by_key(|(l, _)| *l);
        let contiguous = all_ranges.windows(2).all(|k| k[0].1 == k[1].0);
        if !contiguous {
            return Err(eyre!("Does not still cover a contiguous range"));
        }

        // Do we still need this?
        let final_price = self.final_price;
        let total_donated = self.total_donated + rhs.total_donated;
        let current_tick = self.current_tick + rhs.current_tick;

        Ok(Self { current_tick, tick_donations, final_price, total_donated })
    }

    pub fn low_tick(&self) -> Option<Tick> {
        self.tick_donations
            .iter()
            .map(|((l, _), _)| l)
            .min()
            .copied()
    }

    pub fn high_tick(&self) -> Option<Tick> {
        self.tick_donations
            .iter()
            .map(|((_, h), _)| h)
            .max()
            .copied()
    }

    pub fn get_total_donated(&self) -> u128 {
        // if we have ranges.
        if self.tick_donations.is_empty() { self.current_tick } else { self.total_donated }
    }

    pub fn far_tick(&self, start_tick: Tick) -> Option<Tick> {
        let mut all_ranges = self.tick_donations.keys().collect::<Vec<_>>();
        all_ranges.sort_by_key(|(l, _)| *l);
        if all_ranges.len() == 1 {
            // If we only have one range, then short circuit
            return Some(all_ranges[0].0);
        } else if all_ranges[0].0 <= start_tick && start_tick < all_ranges[0].1 {
            // If the first range is where we're at, the last range is our 'far' range
            return all_ranges.last().map(|r| r.0);
        } else if let Some(last) = all_ranges.last() {
            if last.0 <= start_tick && start_tick < last.1 {
                // Otherwise, if the last range is where we're at, the first range is our 'far'
                // range
                return Some(all_ranges[0].0);
            }
        }
        // Any other case and we are in some kinda bad way
        None
    }

    pub fn donate_and_remainder(&self, net_swap: &PoolPriceVec) -> (Self, Option<Self>) {
        let start_tick = net_swap.start_bound.tick;
        let end_tick = net_swap.end_bound.tick;
        let (low, high) = low_to_high(&start_tick, &end_tick);
        // Split everything into two possibilities - ranges within the vec and ranges
        // outside the vec ranges below
        let (within_vec, mut outside): (HashMap<_, _>, HashMap<_, _>) = self
            .tick_donations
            .iter()
            .filter(|t| t.1.0)
            // Copy our data so we can store it in new structures
            .map(|((l, h), q)| ((*l, *h), *q))
            // Low tick of the vec is less than the segment's high bound, and the high tick of the
            // vec is less than or equal to the segment's low bound
            .partition(|((l, h), _)| *low < *h && *high >= *l);

        let second_swap = if !outside.is_empty() {
            let (cur_low, cur_high) =
                (net_swap.end_bound.liq_range.lower_tick, net_swap.end_bound.liq_range.upper_tick);
            outside.insert((cur_low, cur_high), (true, 0));

            Some(Self {
                current_tick:   0,
                final_price:    self.final_price,
                tick_donations: outside,
                total_donated:  self.total_donated
            })
        } else {
            None
        };

        let first_swap = Self {
            current_tick:   self.current_tick,
            final_price:    self.final_price,
            tick_donations: within_vec,
            total_donated:  self.total_donated
        };
        (first_swap, second_swap)
    }
}
