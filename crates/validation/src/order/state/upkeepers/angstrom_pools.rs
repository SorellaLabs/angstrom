use std::collections::HashMap;

use alloy_primitives::{Address, FixedBytes};
use angstrom_types::{primitive::PoolId, sol_bindings::sol::AssetIndex};

pub type PoolIdWithDirection = (bool, PoolId);

/// asset indexes is the concatenation of the two assets
pub type AssetIndexes = u32;
pub struct AngstromPools(HashMap<AssetIndexes, PoolIdWithDirection>);

impl AngstromPools {
    pub fn new(setup: HashMap<AssetIndexes, PoolIdWithDirection>) -> Self {
        AngstromPools(setup)
    }

    pub fn order_info(
        &self,
        currency_in: AssetIndex,
        currency_out: AssetIndex
    ) -> Option<PoolIdWithDirection> {
        tracing::debug!(shit=?self.0);
        self.0
            .get(&self.get_key(currency_in, currency_out))
            .copied()
    }

    #[inline(always)]
    fn get_key(&self, currency_in: AssetIndex, currency_out: AssetIndex) -> AssetIndexes {
        let mut bytes = [0u8; 4];
        bytes[0..2].copy_from_slice(&currency_in.to_be_bytes());
        bytes[2..4].copy_from_slice(&currency_out.to_be_bytes());

        u32::from_be_bytes(bytes)
    }
}
