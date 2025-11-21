use std::collections::HashMap;

use alloy_primitives::{Address, Log, aliases::U24, keccak256};
use alloy_sol_types::SolValue;
use angstrom_types_constants::PoolId;

use crate::contract_bindings::{
    angstrom::Angstrom,
    pool_manager::PoolManager::{self, Initialize},
    position_manager::PositionManager
};

pub type PoolIdWithDirection = (bool, PoolId);

/// just a placeholder type so i can implement the general architecture
#[derive(Debug, Clone, Copy)]
pub struct NewInitializedPool {
    pub currency_in:  Address,
    pub currency_out: Address,
    pub id:           PoolId
}

impl From<Log<Initialize>> for NewInitializedPool {
    fn from(value: Log<Initialize>) -> Self {
        Self {
            currency_in:  value.currency0,
            currency_out: value.currency1,
            id:           value.id
        }
    }
}

macro_rules! pool_key_to_id {
    ($contract:ident) => {
        impl From<$contract::PoolKey> for PoolId {
            fn from(value: $contract::PoolKey) -> Self {
                keccak256(value.abi_encode())
            }
        }

        impl From<&$contract::PoolKey> for PoolId {
            fn from(value: &$contract::PoolKey) -> Self {
                keccak256(value.abi_encode())
            }
        }

        impl Copy for $contract::PoolKey {}
    };
}

pool_key_to_id!(PoolManager);
pool_key_to_id!(PositionManager);
pool_key_to_id!(Angstrom);

#[derive(Debug, Default, Clone)]
pub struct UniswapPoolRegistry {
    pub pools:          HashMap<PoolId, Angstrom::PoolKey>,
    pub conversion_map: HashMap<PoolId, PoolId>
}

impl UniswapPoolRegistry {
    pub fn get(&self, pool_id: &PoolId) -> Option<&Angstrom::PoolKey> {
        self.pools.get(pool_id)
    }

    pub fn pools(&self) -> HashMap<PoolId, Angstrom::PoolKey> {
        self.pools.clone()
    }

    pub fn private_keys(&self) -> impl Iterator<Item = PoolId> + '_ {
        self.conversion_map.values().copied()
    }

    pub fn public_keys(&self) -> impl Iterator<Item = PoolId> + '_ {
        self.conversion_map.keys().copied()
    }
}

impl From<Vec<Angstrom::PoolKey>> for UniswapPoolRegistry {
    fn from(pools: Vec<Angstrom::PoolKey>) -> Self {
        let pubmap = pools
            .iter()
            .map(|pool_key| {
                let pool_id = PoolId::from(*pool_key);
                (pool_id, *pool_key)
            })
            .collect();

        let priv_map = pools
            .into_iter()
            .map(|mut pool_key| {
                let pool_id_pub = PoolId::from(pool_key);
                pool_key.fee = U24::from(0x800000);
                let pool_id_priv = PoolId::from(pool_key);
                (pool_id_pub, pool_id_priv)
            })
            .collect();
        Self { pools: pubmap, conversion_map: priv_map }
    }
}
