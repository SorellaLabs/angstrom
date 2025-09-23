use std::{collections::VecDeque, ops::Add};

use alloy_primitives::{U160, U256, aliases::I24};
use revm_primitives::keccak256;

#[derive(Clone, Debug)]
pub enum DonationType {
    Below { high_tick: i32, donation: u128, liquidity: u128 },
    Above { low_tick: i32, donation: u128, liquidity: u128 },
    Current { final_tick: i32, donation: u128, liquidity: u128 }
}

impl DonationType {
    pub fn current(final_tick: i32, donation: u128, liquidity: u128) -> Self {
        Self::Current { final_tick, donation, liquidity }
    }

    pub fn below(high_tick: i32, donation: u128, liquidity: u128) -> Self {
        Self::Below { high_tick, donation, liquidity }
    }

    pub fn above(low_tick: i32, donation: u128, liquidity: u128) -> Self {
        Self::Above { low_tick, donation, liquidity }
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
            DonationType::Above { donation: d, low_tick, liquidity } => {
                DonationType::above(*low_tick, d + rhs, *liquidity)
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
