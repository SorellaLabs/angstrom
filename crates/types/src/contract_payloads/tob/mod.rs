use std::collections::HashMap;

use alloy::primitives::{aliases::I24, U256};
use eyre::eyre;
use uniswap_v3_math::tick_math::get_tick_at_sqrt_ratio;

use super::rewards::RewardsUpdate;
use crate::{
    matching::uniswap::{PoolSnapshot, Quantity, Tick},
    sol_bindings::{grouped_orders::OrderWithStorageData, rpc_orders::TopOfBlockOrder}
};

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ToBOutcome {
    pub start_tick:      i32,
    pub end_tick:        i32,
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
        let total_cost: u128 = pricevec.input();
        if total_cost > tob.quantity_in {
            return Err(eyre!("Not enough input to cover the transaction"));
        }
        let leftover = tob.quantity_in - total_cost;
        let donation = pricevec.donation(leftover);

        let rewards = Self {
            start_tick:      snapshot.current_price().tick(),
            end_tick:        get_tick_at_sqrt_ratio(U256::from(pricevec.end_bound.price)).unwrap(),
            start_liquidity: snapshot.current_price().liquidity(),
            tribute:         U256::from(donation.tribute),
            total_cost:      U256::from(pricevec.input()),
            total_reward:    U256::from(donation.total_donated),
            tick_donations:  donation.tick_donations
        };
        tracing::info!(?rewards);
        Ok(rewards)
    }

    pub fn to_rewards_update(&self) -> RewardsUpdate {
        let mut donations = self.tick_donations.iter().collect::<Vec<_>>();
        if self.start_tick <= self.end_tick {
            // Will sort from lowest to highest (donations[0] will be the lowest tick
            // number)
            donations.sort_by_key(|f| f.0);
        } else {
            // Will sort from highest to lowest (donations[0] will be the highest tick
            // number)
            donations.sort_by_key(|f| std::cmp::Reverse(f.0));
        }
        // Each reward value is the cumulative sum of the rewards before it
        let quantities = donations
            .iter()
            .scan(U256::ZERO, |state, (_tick, q)| {
                *state += **q;
                Some(u128::try_from(*state).unwrap())
            })
            .collect::<Vec<_>>();
        tracing::trace!(donations = ?donations, "Donations dump");
        let start_tick = I24::try_from(self.start_tick).unwrap_or_default();

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

#[cfg(test)]
mod test {
    use std::collections::HashMap;

    use alloy_primitives::{aliases::I24, U256};

    use super::ToBOutcome;
    use crate::contract_payloads::rewards::RewardsUpdate;

    #[test]
    fn sorts_correctly() {
        let donations = HashMap::from([
            (100, U256::from(123_u128)),
            (110, U256::from(456_u128)),
            (120, U256::from(789_u128))
        ]);

        // Upwards update order checking
        let upwards_update = ToBOutcome {
            start_tick: 100,
            end_tick: 120,
            tick_donations: donations.clone(),
            ..Default::default()
        }
        .to_rewards_update();
        let RewardsUpdate::MultiTick {
            start_tick: upwards_start_tick,
            quantities: upwards_quantities,
            ..
        } = upwards_update
        else {
            panic!("Upwards update was single-tick");
        };

        assert_eq!(
            upwards_quantities[0], 123_u128,
            "Upwards update did not have first quantity at lowest tick"
        );
        assert_eq!(
            upwards_start_tick,
            I24::unchecked_from(100),
            "Upwards update did not start at lowest tick"
        );

        // Downwards update order checking
        let downwards_update = ToBOutcome {
            start_tick: 120,
            end_tick: 100,
            tick_donations: donations.clone(),
            ..Default::default()
        }
        .to_rewards_update();
        let RewardsUpdate::MultiTick {
            start_tick: downwards_start_tick,
            quantities: downwards_quantities,
            ..
        } = downwards_update
        else {
            panic!("Downwards update was single-tick");
        };
        assert_eq!(
            downwards_quantities[0], 789_u128,
            "Downwards update did not have first quantity at highest tick"
        );
        assert_eq!(
            downwards_start_tick,
            I24::unchecked_from(120),
            "Downwards update did not start at highest tick"
        );
    }
}
