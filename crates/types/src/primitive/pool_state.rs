use alloy::{
    primitives::{Address, FixedBytes, Log, keccak256},
    sol_types::SolValue
};

use crate::contract_bindings::{
    angstrom::Angstrom,
    pool_manager::PoolManager::{self, Initialize},
    position_manager::PositionManager
};

pub type PoolId = FixedBytes<32>;

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
