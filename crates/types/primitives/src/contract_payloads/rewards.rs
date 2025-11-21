use alloy_primitives::{U160, aliases::I24};
use pade_macro::{PadeDecode, PadeEncode};
use serde::{Deserialize, Serialize};

use super::{Asset, Pair};

#[derive(
    Debug, PadeEncode, PadeDecode, Clone, PartialEq, Eq, Ord, PartialOrd, Serialize, Deserialize,
)]
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

#[derive(
    Debug, PadeEncode, PadeDecode, Clone, PartialEq, Eq, Ord, PartialOrd, Serialize, Deserialize,
)]
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
