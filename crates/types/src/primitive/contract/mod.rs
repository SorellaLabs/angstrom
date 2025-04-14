use std::{collections::HashMap, fmt::Debug, hash::Hash};

use alloy::{
    dyn_abi::Eip712Domain,
    primitives::{Address, aliases::U24},
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

// internal anvil testnet
#[cfg(all(feature = "testnet", not(feature = "testnet-sepolia")))]
pub const TESTNET_ANGSTROM_ADDRESS: Address =
    alloy::primitives::address!("293954613283cC7B82BfE9676D3cc0fb0A58fAa0");

#[cfg(all(feature = "testnet", not(feature = "testnet-sepolia")))]
pub const TESTNET_POOL_MANAGER_ADDRESS: Address =
    alloy::primitives::address!("48bC5A530873DcF0b890aD50120e7ee5283E0112");

#[cfg(all(feature = "testnet", not(feature = "testnet-sepolia")))]
pub const ANGSTROM_DOMAIN: Eip712Domain = eip712_domain!(
    name: "Angstrom",
    version: "v1",
    chain_id: 1,
    verifying_contract: TESTNET_ANGSTROM_ADDRESS,
);

// sepolia testnet
#[cfg(all(not(feature = "testnet"), feature = "testnet-sepolia"))]
pub const TESTNET_ANGSTROM_ADDRESS: Address =
    alloy::primitives::address!("9D0ce8B3DF426008c4a4E74E7845B1bffF346a90");

#[cfg(all(not(feature = "testnet"), feature = "testnet-sepolia"))]
pub const TESTNET_POOL_MANAGER_ADDRESS: Address =
    alloy::primitives::address!("E03A1074c86CFeDd5C142C4F04F1a1536e203543");

#[cfg(all(not(feature = "testnet"), feature = "testnet-sepolia"))]
pub const TESTNET_POSITION_MANAGER_ADDRESS: Address =
    alloy::primitives::address!("429ba70129df741B2Ca2a85BC3A2a3328e5c09b4");

#[cfg(all(not(feature = "testnet"), feature = "testnet-sepolia"))]
pub const TESTNET_CONTROLLER_V1_ADDRESS: Address =
    alloy::primitives::address!("4De4326613020a00F5545074bC578C87761295c7");

#[cfg(all(not(feature = "testnet"), feature = "testnet-sepolia"))]
pub const ANGSTROM_DOMAIN: Eip712Domain = eip712_domain!(
    name: "Angstrom",
    version: "v1",
    chain_id: 11155111,
    verifying_contract: TESTNET_ANGSTROM_ADDRESS,
);

// odd cases that we need to handle but should be unreachable.
#[cfg(all(feature = "testnet", feature = "testnet-sepolia"))]
pub const TESTNET_ANGSTROM_ADDRESS: Address =
    alloy::primitives::address!("293954613283cC7B82BfE9676D3cc0fb0A58fAa0");

#[cfg(all(not(feature = "testnet"), not(feature = "testnet-sepolia")))]
pub const ANGSTROM_DOMAIN: Eip712Domain = eip712_domain!(
    name: "Angstrom",
    version: "not both",
    chain_id: 1,
);
#[cfg(all(feature = "testnet", feature = "testnet-sepolia"))]
pub const ANGSTROM_DOMAIN: Eip712Domain = eip712_domain!(
    name: "Angstrom",
    version: "both",
    chain_id: 1,
    verifying_contract: TESTNET_ANGSTROM_ADDRESS,

);
#[cfg(all(feature = "testnet", feature = "testnet-sepolia"))]
pub const TESTNET_POOL_MANAGER_ADDRESS: Address =
    alloy::primitives::address!("48bC5A530873DcF0b890aD50120e7ee5283E0112");

#[derive(Debug, Default, Clone)]
pub struct UniswapPoolRegistry {
    pub pools:          HashMap<PoolId, PoolKey>,
    pub conversion_map: HashMap<PoolId, PoolId>
}

impl UniswapPoolRegistry {
    pub fn get(&self, pool_id: &PoolId) -> Option<&PoolKey> {
        self.pools.get(pool_id)
    }

    pub fn pools(&self) -> HashMap<PoolId, PoolKey> {
        self.pools.clone()
    }

    pub fn private_keys(&self) -> impl Iterator<Item = PoolId> + '_ {
        self.conversion_map.values().copied()
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
