use alloy_primitives::{Address, Log};
pub use angstrom_types_contracts::PoolId;
use angstrom_types_contracts::{
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
