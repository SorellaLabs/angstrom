use alloy::{
    primitives::{Address, Bytes, B256},
    sol
};
use pade_macro::{PadeDecode, PadeEncode};
use reth_primitives::keccak256;
use serde::{Deserialize, Serialize};

use crate::{
    contract_payloads::tob::{Asset, RewardsUpdate},
    sol_bindings::{
        grouped_orders::OrderWithStorageData, rpc_orders::TopOfBlockOrder as RpcTopOfBlockOrder
    }
};

// This currently exists in types::sol_bindings as well, but that one is
// outdated so I'm building a new one here for now and then migrating
#[derive(
    PadeEncode, PadeDecode, Clone, Default, Debug, Hash, PartialEq, Eq, Serialize, Deserialize,
)]
pub struct TopOfBlockOrder {
    pub use_internal:    bool,
    pub quantity_in:     u128,
    pub quantity_out:    u128,
    pub asset_in_index:  u16,
    pub asset_out_index: u16,
    pub recipient:       Option<Address>,
    pub hook_data:       Option<Bytes>,
    pub signature:       Bytes
}

impl TopOfBlockOrder {
    // eip-712 hash_struct
    pub fn order_hash(&self) -> B256 {
        keccak256(&self.signature)
    }

    pub fn of(
        internal: &OrderWithStorageData<RpcTopOfBlockOrder>,
        asset_in_index: u16,
        asset_out_index: u16
    ) -> Self {
        let quantity_in = internal.quantityIn;
        let quantity_out = internal.quantityOut;
        let recipient = Some(internal.recipient);
        let hook_data = Some(internal.hookPayload.clone());
        let signature = internal.meta.signature.clone();
        Self {
            use_internal: false,
            quantity_in,
            quantity_out,
            asset_in_index,
            asset_out_index,
            recipient,
            hook_data,
            signature
        }
    }
}

#[derive(Debug, PadeEncode, PadeDecode)]
pub struct StandingValidation {
    nonce:    u64,
    // 40 bits wide in reality
    #[pade_width(5)]
    deadline: u64
}

#[derive(Debug, PadeEncode, PadeDecode)]
pub enum OrderQuantities {
    Exact { quantity: u128 },
    Partial { min_quantity_in: u128, max_quantity_in: u128, filled_quantity: u128 }
}

#[derive(Debug, PadeEncode, PadeDecode)]
pub struct UserOrder {
    pub use_internal:        bool,
    pub pair_index:          u16,
    pub min_price:           alloy::primitives::U256,
    pub recipient:           Option<Address>,
    pub hook_data:           Option<Bytes>,
    pub a_to_b:              bool,
    pub standing_validation: Option<StandingValidation>,
    pub order_quantities:    OrderQuantities,
    pub exact_in:            bool,
    pub signature:           Bytes
}

impl UserOrder {
    pub fn order_hash(&self) -> B256 {
        keccak256(&self.signature)
    }
}

sol! {
    #[derive(Debug, PadeEncode, PadeDecode)]
    struct Pair {
        uint16 t1_idx;
        uint16 t0_idx;
        uint256 uniswap_price;
    }

    #[derive(Debug, PadeEncode, PadeDecode)]
    struct PoolUpdate {
        uint16 asset_in_index;
        uint16 asset_out_index;
        uint128 swap_in_quantity;
        RewardsUpdate rewards_update;
    }
}

#[derive(Debug, PadeEncode, PadeDecode)]
pub struct AngstromBundle {
    pub assets:              Vec<Asset>,
    pub pairs:               Vec<Pair>,
    pub pool_updates:        Vec<PoolUpdate>,
    pub top_of_block_orders: Vec<TopOfBlockOrder>,
    pub user_orders:         Vec<UserOrder>
}

impl AngstromBundle {
    pub fn get_order_hashes(&self) -> impl Iterator<Item = B256> + '_ {
        self.top_of_block_orders
            .iter()
            .map(|order| order.order_hash())
            .chain(self.user_orders.iter().map(|order| order.order_hash()))
    }
}

impl AngstromBundle {
    pub fn new(
        assets: Vec<Asset>,
        pairs: Vec<Pair>,
        pool_updates: Vec<PoolUpdate>,
        top_of_block_orders: Vec<TopOfBlockOrder>,
        user_orders: Vec<UserOrder>
    ) -> Self {
        Self { assets, pairs, pool_updates, top_of_block_orders, user_orders }
    }
}
