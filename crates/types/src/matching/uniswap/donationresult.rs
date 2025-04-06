use std::collections::HashMap;

use eyre::eyre;

use super::{PoolPriceVec, Tick};
use crate::matching::{SqrtPriceX96, math::low_to_high};

#[derive(Debug)]
pub struct DonationResult {
    /// HashMap from liquidity range bounds `(lower_tick, upper_tick)` to
    /// donation quantity in T0
    pub tick_donations: HashMap<(Tick, Tick), u128>,
    pub final_price:    SqrtPriceX96,
    /// Total amount of donation in T0
    pub total_donated:  u128,
    /// Total amount of "tribute" taken by Angstrom in T0
    pub tribute:        u128
}

impl DonationResult {
    /// Combine this donation result with another donation result.  Donations to
    /// the same liquidity segment will be summed.  This method will also check
    /// to ensure that our output is a set of donations to a contiguous range.
    pub fn combine(&self, rhs: &Self) -> eyre::Result<Self> {
        // Merge our two HashMaps
        let mut tick_donations = HashMap::new();
        for (k, v) in self.tick_donations.iter().chain(rhs.tick_donations.iter()) {
            tick_donations
                .entry(*k)
                .and_modify(|e| *e += *v)
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
        let tribute = self.tribute + rhs.tribute;

        Ok(Self { tick_donations, final_price, total_donated, tribute })
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
            .filter(|t| *t.1 != 0)
            // Copy our data so we can store it in new structures
            .map(|((l, h), q)| ((*l, *h), *q))
            // Low tick of the vec is less than the segment's high bound, and the high tick of the
            // vec is less than or equal to the segment's low bound
            .partition(|((l, h), _)| *low < *h && *high >= *l);

        let second_swap = if !outside.is_empty() {
            let (cur_low, cur_high) =
                (net_swap.end_bound.liq_range.lower_tick, net_swap.end_bound.liq_range.upper_tick);
            outside.insert((cur_low, cur_high), 0);
            Some(Self {
                final_price:    self.final_price,
                tick_donations: outside,
                total_donated:  self.total_donated,
                tribute:        self.tribute
            })
        } else {
            None
        };

        let first_swap = Self {
            final_price:    self.final_price,
            tick_donations: within_vec,
            total_donated:  self.total_donated,
            tribute:        self.tribute
        };
        (first_swap, second_swap)
    }
}
