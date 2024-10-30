use std::collections::HashMap;

use alloy::{
    dyn_abi::Eip712Domain,
    sol,
    sol_types::{eip712_domain, SolValue}
};
use alloy_primitives::keccak256;

mod angstrom;
pub use angstrom::{Angstrom::*, *};

sol! {
#![sol(all_derives = true)]
ERC20,
"src/primitive/contract/ERC20.json"}

pub use ERC20::*;

use crate::primitive::PoolId;

// The `eip712_domain` macro lets you easily define an EIP-712 domain
// object :)
pub const ANGSTROM_DOMAIN: Eip712Domain = eip712_domain!(
   name: "Angstrom",
   version: "1",
);

impl From<PoolKey> for PoolId {
    fn from(pool_key: PoolKey) -> Self {
        let encoded = (
            pool_key.currency0,
            pool_key.currency1,
            pool_key.fee,
            pool_key.tickSpacing,
            pool_key.hooks
        )
            .abi_encode();
        PoolId::from(keccak256(&encoded))
    }
}

#[derive(Default, Clone)]
pub struct UniswapPoolRegistry {
    pools: HashMap<PoolId, PoolKey>
}
impl UniswapPoolRegistry {
    pub fn get(&self, pool_id: &PoolId) -> Option<&PoolKey> {
        self.pools.get(pool_id)
    }

    pub fn pools(&self) -> HashMap<PoolId, PoolKey> {
        self.pools.clone()
    }
}
impl From<Vec<PoolKey>> for UniswapPoolRegistry {
    fn from(pools: Vec<PoolKey>) -> Self {
        let pools = pools
            .into_iter()
            .map(|pool_key| {
                let pool_id = PoolId::from(pool_key.clone());
                (pool_id, pool_key)
            })
            .collect();
        Self { pools }
    }
}
