use std::{collections::VecDeque, ops::Add};

use alloy::primitives::{U160, U256, aliases::I24};
use revm_primitives::keccak256;

use crate::contract_payloads::rewards::RewardsUpdate;

#[derive(Clone, Debug)]
pub enum DonationType {
    Below { high_tick: i32, donation: u128, liquidity: u128 },
    Above { low_tick: i32, donation: u128, liquidity: u128, tick_spacing: i32 },
    Current { final_tick: i32, donation: u128, liquidity: u128 }
}

impl DonationType {
    pub fn current(final_tick: i32, donation: u128, liquidity: u128) -> Self {
        Self::Current { final_tick, donation, liquidity }
    }

    pub fn below(high_tick: i32, donation: u128, liquidity: u128) -> Self {
        Self::Below { high_tick, donation, liquidity }
    }

    pub fn above(low_tick: i32, donation: u128, liquidity: u128, tick_spacing: i32) -> Self {
        Self::Above { low_tick, donation, liquidity, tick_spacing }
    }

    pub fn donation(&self) -> u128 {
        match self {
            Self::Current { donation, .. } => *donation,
            Self::Above { donation, .. } => *donation,
            Self::Below { donation, .. } => *donation
        }
    }

    pub fn liquidity(&self) -> u128 {
        match self {
            Self::Current { liquidity, .. } => *liquidity,
            Self::Above { liquidity, .. } => *liquidity,
            Self::Below { liquidity, .. } => *liquidity
        }
    }

    pub fn tick(&self) -> i32 {
        match self {
            Self::Current { final_tick, .. } => *final_tick,
            Self::Above { low_tick, .. } => *low_tick,
            Self::Below { high_tick, .. } => *high_tick
        }
    }
}

impl Add<u128> for &DonationType {
    type Output = DonationType;

    fn add(self, rhs: u128) -> Self::Output {
        match self {
            DonationType::Below { donation: d, high_tick, liquidity } => {
                DonationType::below(*high_tick, d + rhs, *liquidity)
            }
            DonationType::Above { donation: d, low_tick, liquidity, tick_spacing } => {
                DonationType::above(*low_tick, d + rhs, *liquidity, *tick_spacing)
            }
            DonationType::Current { final_tick, donation: d, liquidity } => {
                DonationType::current(*final_tick, d + rhs, *liquidity)
            }
        }
    }
}

impl Add<&DonationType> for &DonationType {
    type Output = DonationType;

    fn add(self, rhs: &DonationType) -> Self::Output {
        rhs + self.donation()
    }
}

#[derive(Clone, Debug)]
pub struct DonationCalculation {
    // the amount to donate to the current tick of the pool.
    // pub current_tick: u128,
    // what direction the donate is going
    // pub direction: bool,
    // pub current_tick: i32,
    // the amount to donate at the next tick increments
    pub donations:    VecDeque<DonationType>,
    pub current_tick: i32,
    /// Index of the "Current" element in our vector
    pub break_idx:    usize,

    /// Hash of target donation ticks to (quantity, liquidity)
    // pub rest:          HashMap<i32, (u128, u128)>,
    pub total_donated: u128,
    pub tick_spacing:  i32
}

impl DonationCalculation {
    pub fn from_vec(vec: &[DonationType], tick_spacing: i32) -> eyre::Result<Self> {
        // If we're coming from an empty vec, return an empty DonationCalculation
        if vec.is_empty() {
            return Ok(Self {
                donations: VecDeque::new(),
                current_tick: 0,
                break_idx: 0,
                total_donated: 0,
                tick_spacing
            });
        }
        assert!(
            vec.iter()
                .any(|x| matches!(x, DonationType::Current { .. })),
            "Vec does not have a 'Current' element!"
        );
        let (donations, break_idx) = match vec.first() {
            Some(DonationType::Above { .. }) => {
                (vec.iter().cloned().rev().collect::<VecDeque<_>>(), 0)
            }
            Some(_) => (vec.iter().cloned().collect::<VecDeque<_>>(), vec.len() - 1),
            None => (VecDeque::new(), 0)
        };

        let current_tick = if let Some(DonationType::Current { final_tick, .. }) =
            vec.get(vec.len().saturating_sub(1))
        {
            *final_tick
        } else {
            0
        };
        // If the first element in our vec is an Above, we need to reverse the vec
        let total_donated = vec.iter().fold(0_u128, |acc, e| acc + e.donation());
        Ok(Self { donations, break_idx, current_tick, total_donated, tick_spacing })
    }

    fn checksum_iter<'a, T: Clone + Iterator<Item = &'a DonationType>>(
        donations: T,
        len: usize
    ) -> U160 {
        let tick_iter = donations.clone().take(len - 1).map(|d| d.tick());
        let liq_iter = donations.clone().skip(1).map(|d| d.liquidity());
        let checksum_bytes = tick_iter
            .zip(liq_iter)
            .fold([0u8; 32], |acc, (tick, liquidity)| {
                let tick_bytes: [u8; 3] = I24::unchecked_from(tick).to_be_bytes();
                let hash_input = [acc.as_slice(), &liquidity.to_be_bytes(), &tick_bytes].concat();
                *keccak256(&hash_input)
            });
        U160::from(U256::from_be_bytes(checksum_bytes) >> 96)
    }

    pub fn into_reward_updates(&self) -> (RewardsUpdate, Option<RewardsUpdate>) {
        match (self.break_idx, self.donations.len()) {
            (_, 0) | (_, 1) => {
                // Whatever our break_idx is, if we have just one donation it must be just the
                // current tick
                let (amount, expected_liquidity) = if let Some(d) = self.donations.front() {
                    (d.donation(), d.liquidity())
                } else {
                    (0, 0)
                };
                (RewardsUpdate::CurrentOnly { amount, expected_liquidity }, None)
            }
            (0, len) => {
                // If the break_idx is 0, the entire donation vec is one side - above
                let (mut start_tick, start_liquidity) = (
                    I24::unchecked_from(self.donations[len - 1].tick()),
                    self.donations[len - 1].liquidity()
                );
                let quantities = self.donations.iter().rev().map(|d| d.donation()).collect();
                let reward_checksum = Self::checksum_iter(self.donations.iter().rev(), len);

                // for rewarding above, make sure the start tick lies on the upper bound of the
                // tick spacing
                while start_tick % I24::unchecked_from(self.tick_spacing) != I24::ZERO {
                    start_tick += I24::ONE;
                }
                (
                    RewardsUpdate::MultiTick {
                        start_tick,
                        start_liquidity,
                        quantities,
                        reward_checksum
                    },
                    None
                )
            }
            (b, len) if b == (len - 1) => {
                // If the break_idx is the max index, the entire donation vec is below
                let (start_tick, start_liquidity) =
                    (I24::unchecked_from(self.donations[0].tick()), self.donations[0].liquidity());
                let quantities = self.donations.iter().map(|d| d.donation()).collect();
                let reward_checksum = Self::checksum_iter(self.donations.iter(), len);
                (
                    RewardsUpdate::MultiTick {
                        start_tick,
                        start_liquidity,
                        quantities,
                        reward_checksum
                    },
                    None
                )
            }
            (..) => {
                // We have some mixed split to do
                let mut below = self.donations.clone();
                let self_and_above = below.split_off(self.break_idx);
                below.push_back(self_and_above[0].clone());

                // Self_and_above
                let saa_last = self_and_above.len() - 1;
                let (mut start_tick, start_liquidity) = (
                    I24::unchecked_from(self_and_above[saa_last].tick()),
                    self_and_above[saa_last].liquidity()
                );
                let quantities = self_and_above.iter().rev().map(|d| d.donation()).collect();
                let reward_checksum =
                    Self::checksum_iter(self_and_above.iter(), self_and_above.len());

                // for rewarding above, make sure the start tick lies on the upper bound of the
                // tick spacing
                while start_tick % I24::unchecked_from(self.tick_spacing) != I24::ZERO {
                    start_tick += I24::ONE;
                }

                // Below
                let (below_start_tick, below_start_liquidity) =
                    (I24::unchecked_from(below[0].tick()), below[0].liquidity());
                let mut below_quantities: Vec<u128> = below.iter().map(|d| d.donation()).collect();
                // Given that we rewarded "current" above, we can't reward it here
                if let Some(last_q) = below_quantities.last_mut() {
                    *last_q = 0;
                }
                let below_reward_checksum = Self::checksum_iter(below.iter(), below.len());
                (
                    RewardsUpdate::MultiTick {
                        start_tick,
                        start_liquidity,
                        quantities,
                        reward_checksum
                    },
                    Some(RewardsUpdate::MultiTick {
                        start_tick:      below_start_tick,
                        start_liquidity: below_start_liquidity,
                        quantities:      below_quantities,
                        reward_checksum: below_reward_checksum
                    })
                )
            }
        }
    }

    pub fn total_donated(&self) -> u128 {
        self.total_donated
    }
}

impl Add<&[DonationType]> for &DonationCalculation {
    type Output = DonationCalculation;

    fn add(self, rhs: &[DonationType]) -> Self::Output {
        let mut donations = self.donations.clone();
        let mut total_donated = self.total_donated;
        let mut rel_idx = self.break_idx + 1;
        let mut current_tick = self.current_tick;
        for i in rhs {
            // We always update our total_donated with the donation amount from our incoming
            // vec
            total_donated += i.donation();

            // If our incoming donation event is "Current", this will be the end and we
            // should set our final tick to the final tick described there
            if let DonationType::Current { final_tick, .. } = i {
                current_tick = *final_tick;
            }
            // Check to see if we are going to merge or extend
            // if if rel_idx - 1 gets us something, we're merging
            // if rel_idx - 1 gets us nothing, we're extending off the front
            // If we've hit the new "Current" then update our final_tick to be that tick
            if rel_idx == 0 {
                // We're moving downwards and off the front edge of our storage vec, so we will
                // just be pushing all future elements to the front of the vec and we don't need
                // to move rel_idx anymore
                donations.push_front(i.clone());
                // We no longer need to adjust rel_idx
                continue;
            } else if let Some(entry) = donations.get_mut(rel_idx - 1) {
                // If we're already pointing at an existing entry it needs to be combined and
                // flipped to the new entry type
                *entry = &*entry + i;
                // We move rel_idx based on the type of new entry we're at
            } else {
                // If we're up off the back edge of our storage vec, we will be pushing all
                // future elements to the end of the vec, but we still do want to move rel_idx
                // to point at the right end element
                donations.push_back(i.clone());
            }
            // Ajdust our pointer.  When we hit `Current` in the new vec, we're done and we
            // should leave rel_idx pointing there
            match i {
                DonationType::Above { .. } => rel_idx -= 1,
                DonationType::Below { .. } => rel_idx += 1,
                DonationType::Current { .. } => ()
            }
        }
        // We use saturating_sub because if our rel_idx is 0, that's where it should
        // stay
        let break_idx = rel_idx.saturating_sub(1);
        DonationCalculation {
            donations,
            total_donated,
            break_idx,
            current_tick,
            tick_spacing: self.tick_spacing
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DonationCalculation, DonationType};

    #[test]
    pub fn constructs_from_empty_vec() {
        let donation = DonationCalculation::from_vec(&[], 10).unwrap();
        let res = donation.into_reward_updates();
        if let (r, None) = res {
            println!("{r:?}");
        } else {
            panic!("Made the wrong thing");
        }
    }

    #[test]
    pub fn extends_properly() {
        let don_vec = vec![
            DonationType::Below { donation: 100, high_tick: -500, liquidity: 100 },
            DonationType::Below { donation: 100, high_tick: -400, liquidity: 100 },
            DonationType::Below { donation: 100, high_tick: -300, liquidity: 100 },
            DonationType::Current { donation: 100, final_tick: -278, liquidity: 100 },
        ];
        let second_vec = vec![
            DonationType::Below { donation: 100, high_tick: -200, liquidity: 100 },
            DonationType::Below { donation: 100, high_tick: -100, liquidity: 100 },
            DonationType::Below { donation: 100, high_tick: 0, liquidity: 100 },
            DonationType::Current { donation: 100, final_tick: 78, liquidity: 100 },
        ];
        let donation = DonationCalculation::from_vec(&don_vec, 10).unwrap();
        let extended = &donation + &second_vec;
        let _ = extended.into_reward_updates();
    }
}
