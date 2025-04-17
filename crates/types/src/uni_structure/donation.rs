use std::collections::HashMap;

use alloy_primitives::{U16, U160, aliases::I24};

use super::{BaselinePoolState, pool_swap::PoolSwapResult};
use crate::contract_payloads::rewards::RewardsUpdate;

pub struct DonationCalculation {
    // the amount to donate to the current tick of the pool.
    // pub current_tick: u128,
    // what direction the donate is going
    // pub direction: bool,
    // pub current_tick: i32,
    // the amount to donate at the next tick increments
    pub rest:          HashMap<i32, u128>,
    pub total_donated: u128
}

impl DonationCalculation {
    pub fn into_reward_updates(
        &self,
        snapshot: &PoolSwapResult<'_>
    ) -> (RewardsUpdate, Option<RewardsUpdate>) {
        // if self.rest.is_empty() {
        return (
            RewardsUpdate::CurrentOnly {
                amount:             self.total_donated,
                expected_liquidity: snapshot.end_liquidity.current_liquidity
            },
            None
        );
        // }

        // the way we want to do this is tricky.
        let current_tick = snapshot.end_liquidity.current_tick;
        // we want to split this into below and above
        let (mut lower, mut upper): (Vec<(&i32, &u128)>, Vec<(&i32, &u128)>) = self
            .rest
            .iter()
            .partition(|(tick, _)| *tick <= &current_tick);

        // want high to low sorting
        upper.sort_unstable_by_key(|(t, _)| -*t);

        // want low to high sorting.
        lower.sort_unstable_by_key(|(t, _)| *t);

        match (lower.is_empty(), upper.is_empty()) {
            (true, true) => {
                return (
                    RewardsUpdate::CurrentOnly {
                        amount:             self.total_donated,
                        expected_liquidity: snapshot.end_liquidity.current_liquidity
                    },
                    None
                );
            }

            (true, false) => {
                // reward above
                match upper.len() {
                    1 => {
                        return (
                            RewardsUpdate::CurrentOnly {
                                amount:             self.total_donated,
                                expected_liquidity: snapshot.end_liquidity.current_liquidity
                            },
                            None
                        );
                    }
                    _ => {
                        // multi_tick
                        let start_tick = I24::unchecked_from(*upper.first().unwrap().0);
                        let start_liquidity = 0;
                        let donate = RewardsUpdate::MultiTick {
                            start_tick,
                            start_liquidity,
                            quantities: upper.iter().map(|(_, a)| **a).collect(),
                            reward_checksum: U160::default()
                        };

                        return (donate, None);
                    }
                }
            }
            (false, true) => match lower.len() {
                1 => {
                    return (
                        RewardsUpdate::CurrentOnly {
                            amount:             self.total_donated,
                            expected_liquidity: snapshot.end_liquidity.current_liquidity
                        },
                        None
                    );
                }
                _ => {
                    let start_tick = I24::unchecked_from(*lower.first().unwrap().0);
                    let start_liquidity = 0;
                    let donate = RewardsUpdate::MultiTick {
                        start_tick,
                        start_liquidity,
                        quantities: lower.iter().map(|(_, a)| **a).collect(),
                        reward_checksum: U160::default()
                    };

                    return (donate, None);
                }
            },
            (false, false) => {
                // split reward
                let lower = match lower.len() {
                    1 => RewardsUpdate::CurrentOnly {
                        amount:             self.total_donated,
                        expected_liquidity: snapshot.end_liquidity.current_liquidity
                    },
                    _ => {
                        let start_tick = I24::unchecked_from(*lower.first().unwrap().0);
                        let start_liquidity = 0;
                        let donate = RewardsUpdate::MultiTick {
                            start_tick,
                            start_liquidity,
                            quantities: lower.iter().map(|(_, a)| **a).collect(),
                            reward_checksum: U160::default()
                        };

                        donate
                    }
                };

                let upper = match upper.len() {
                    1 => RewardsUpdate::CurrentOnly {
                        amount:             self.total_donated,
                        expected_liquidity: snapshot.end_liquidity.current_liquidity
                    },
                    _ => {
                        // multi_tick
                        let start_tick = I24::unchecked_from(*upper.first().unwrap().0);
                        let start_liquidity = 0;
                        let donate = RewardsUpdate::MultiTick {
                            start_tick,
                            start_liquidity,
                            quantities: upper.iter().map(|(_, a)| **a).collect(),
                            reward_checksum: U160::default()
                        };

                        donate
                    }
                };

                return (lower, Some(upper));
            }
        }
    }

    pub fn merge_rewards(&self, rhs: &Self) -> Self {
        let new_ticks = self.rest.iter().chain(rhs.rest.iter()).fold(
            HashMap::<i32, u128>::new(),
            |mut acc, (tick, am)| {
                acc.entry(*tick).and_modify(|a| *a += am).or_insert(*am);
                acc
            }
        );
        let total_donate = self.total_donated + rhs.total_donated;
        Self { rest: new_ticks, total_donated: total_donate }
    }

    pub fn total_donated(&self) -> u128 {
        self.total_donated
    }
}
