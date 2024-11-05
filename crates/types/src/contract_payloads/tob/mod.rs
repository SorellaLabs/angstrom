use std::collections::HashMap;

use alloy::primitives::{aliases::I24, U256};
use eyre::eyre;

use super::rewards::RewardsUpdate;
use crate::{
    matching::uniswap::{PoolSnapshot, Quantity, Tick},
    sol_bindings::{grouped_orders::OrderWithStorageData, rpc_orders::TopOfBlockOrder}
};

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ToBOutcome {
    pub start_tick:      i32,
    pub start_liquidity: u128,
    pub tribute:         U256,
    pub total_cost:      U256,
    pub total_reward:    U256,
    pub tick_donations:  HashMap<Tick, U256>
}

impl ToBOutcome {
    /// Sum of the donations across all ticks
    pub fn total_donations(&self) -> U256 {
        self.tick_donations
            .iter()
            .fold(U256::ZERO, |acc, (_tick, donation)| acc + donation)
    }

    /// Tick donations plus tribute to determine total value of this outcome
    pub fn total_value(&self) -> U256 {
        self.total_donations() + self.tribute
    }

    pub fn from_tob_and_snapshot(
        tob: &OrderWithStorageData<TopOfBlockOrder>,
        snapshot: &PoolSnapshot
    ) -> eyre::Result<Self> {
        let output = match tob.is_bid {
            true => Quantity::Token0(tob.quantity_out),
            false => Quantity::Token1(tob.quantity_out)
        };
        let pricevec = (snapshot.current_price() - output)?;
        let total_cost: u128 = pricevec.input().saturating_to();
        if total_cost > tob.quantity_in {
            return Err(eyre!("Not enough input to cover the transaction"));
        }
        let leftover = tob.quantity_in - total_cost;
        let donation = pricevec.donation(leftover);
        let rewards = ToBOutcome {
            start_tick:      snapshot.current_price().tick(),
            start_liquidity: snapshot.current_price().liquidity(),
            tribute:         U256::from(donation.tribute),
            total_cost:      pricevec.input(),
            total_reward:    U256::from(donation.total_donated),
            tick_donations:  donation.tick_donations
        };
        Ok(rewards)
    }

    pub fn to_rewards_update(&self) -> RewardsUpdate {
        let mut donations = self.tick_donations.iter().collect::<Vec<_>>();
        // Will sort from lowest to highest (donations[0] will be the lowest tick
        // number)
        donations.sort_by_key(|f| f.0);
        // Each reward value is the cumulative sum of the rewards before it
        let quantities = donations
            .iter()
            .scan(U256::ZERO, |state, (_tick, q)| {
                *state += **q;
                Some(u128::try_from(*state).unwrap())
            })
            .collect::<Vec<_>>();
        let start_tick = I24::try_from(donations.first().map(|(a, _)| *a + 1).unwrap_or_default())
            .unwrap_or_default();
        match quantities.len() {
            0 | 1 => RewardsUpdate::CurrentOnly {
                amount: quantities.first().copied().unwrap_or_default()
            },
            _ => RewardsUpdate::MultiTick {
                start_tick,
                start_liquidity: self.start_liquidity,
                quantities
            }
        }
    }
}
