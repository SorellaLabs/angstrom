use std::collections::HashMap;

use alloy::{
    dyn_abi::Eip712Domain,
    primitives::{aliases::U24, Address},
    sol,
    sol_types::eip712_domain
};

use crate::contract_bindings::angstrom::Angstrom::PoolKey;

sol! {
#![sol(all_derives = true)]
ERC20,
"src/primitive/contract/ERC20.json"}

pub use ERC20::*;

use crate::primitive::PoolId;

pub const TESTNET_ANGSTROM_ADDRESS: Address =
    alloy::primitives::address!("293954613283cC7B82BfE9676D3cc0fb0A58fAa0");

pub const TESTNET_POOL_MANAGER_ADDRESS: Address =
    alloy::primitives::address!("48bC5A530873DcF0b890aD50120e7ee5283E0112");
// The `eip712_domain` macro lets you easily define an EIP-712 domain
// object :)
pub const ANGSTROM_DOMAIN: Eip712Domain = eip712_domain!(
    name: "Angstrom",
    version: "v1",
    chain_id: 1,
    verifying_contract: TESTNET_ANGSTROM_ADDRESS,
);

#[derive(Default, Clone)]
pub struct UniswapPoolRegistry {
    pools:              HashMap<PoolId, PoolKey>,
    pub conversion_map: HashMap<PoolId, PoolId>
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
        let pubmap = pools
            .iter()
            .map(|pool_key| {
                let pool_id = PoolId::from(pool_key.clone());
                (pool_id, pool_key.clone())
            })
            .collect();

        let priv_map = pools
            .into_iter()
            .map(|mut pool_key| {
                let pool_id_pub = PoolId::from(pool_key.clone());
                pool_key.fee = U24::from(0x800000);
                let pool_id_priv = PoolId::from(pool_key.clone());
                (pool_id_pub, pool_id_priv)
            })
            .collect();
        Self { pools: pubmap, conversion_map: priv_map }
    }
}
