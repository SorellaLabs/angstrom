use alloy::primitives::aliases::I24;
use pade_macro::{PadeDecode, PadeEncode};

use super::{Asset, Pair};

#[derive(Debug, PadeEncode, PadeDecode)]
pub enum RewardsUpdate {
    MultiTick { start_tick: I24, start_liquidity: u128, quantities: Vec<u128> },
    CurrentOnly { amount: u128 }
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
