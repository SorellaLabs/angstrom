use alloy::primitives::aliases::I24;
use alloy_primitives::U160;
use itertools::Itertools;
use pade_macro::{PadeDecode, PadeEncode};

use super::{Asset, Pair};
use crate::matching::uniswap::{DonationResult, PoolPriceVec, PoolSnapshot};

#[derive(Debug, PadeEncode, PadeDecode)]
pub enum RewardsUpdate {
    MultiTick {
        start_tick:      I24,
        start_liquidity: u128,
        quantities:      Vec<u128>,
        reward_checksum: U160
    },
    CurrentOnly {
        amount:             u128,
        expected_liquidity: u128
    }
}

impl RewardsUpdate {
    pub fn from_data(
        current_tick: i32,
        bound_tick: i32,
        snapshot: &PoolSnapshot,
        donation_data: &DonationResult
    ) -> eyre::Result<Self> {
        // If our bound is a higher tick value, we're doing this the `from_above` way,
        // otherwise we're coming from below the current tick
        let from_above = bound_tick > current_tick;
        let (low, high) =
            if from_above { (&current_tick, &bound_tick) } else { (&bound_tick, &current_tick) };
        let quantities = donation_data
            .tick_donations
            .iter()
            // Filter for only ranges within our provided bounds
            .filter(|((l, h), _)| *h > *low && *l <= *high)
            // Sort to put them in order by low bound tick - high-to-low if from_above, low-to-high
            // otherwise
            .sorted_by(|((a, _), _), ((b, _), _)| if from_above { b.cmp(a) } else { a.cmp(b) })
            // Map to the quantity
            .filter(|f| f.1.0)
            .map(|(_, q)| q.1)
            .collect::<Vec<_>>();

        let (start_tick, start_liquidity) = snapshot
            .get_range_for_tick(bound_tick)
            .map(|r| (if from_above { r.lower_tick() } else { r.upper_tick() }, r.liquidity()))
            .unwrap_or_default();

        match quantities.len() {
            0 | 1 => Ok(Self::CurrentOnly {
                amount:             quantities.first().copied().unwrap_or(0),
                expected_liquidity: start_liquidity
            }),
            _ => Ok(Self::MultiTick {
                start_tick: I24::try_from(start_tick).unwrap_or_default(),
                start_liquidity,
                quantities,
                reward_checksum: snapshot.checksum_from_ticks(bound_tick, current_tick)?
            })
        }
    }

    pub fn from_vec(pricevec: &PoolPriceVec, total_donation: u128) -> eyre::Result<Self> {
        Self::from_data(
            pricevec.start_bound.tick,
            pricevec.end_bound.tick,
            pricevec.snapshot(),
            &pricevec.t0_donation(total_donation)
        )
    }

    pub fn empty() -> Self {
        Self::CurrentOnly { amount: 0, expected_liquidity: 0 }
    }

    pub fn quantities(&self) -> Vec<u128> {
        match self {
            Self::MultiTick { quantities, .. } => quantities.clone(),
            Self::CurrentOnly { amount, .. } => vec![*amount]
        }
    }
}

#[derive(Debug, PadeEncode, PadeDecode)]
pub struct PoolUpdate {
    pub zero_for_one:     bool,
    pub pair_index:       u16,
    pub swap_in_quantity: u128,
    pub rewards_update:   RewardsUpdate
}

#[derive(PadeEncode, Debug)]
pub struct MockContractMessage {
    pub assets: Vec<Asset>,
    pub pairs:  Vec<Pair>,
    pub update: PoolUpdate
}
