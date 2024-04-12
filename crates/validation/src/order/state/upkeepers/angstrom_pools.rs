use std::collections::HashMap;

use alloy_primitives::{Address, FixedBytes};
use angstrom_types::primitive::PoolId;

pub type PoolIdWithDirection = (bool, PoolId);

pub struct AngstromPools(HashMap<FixedBytes<40>, PoolIdWithDirection>);

impl AngstromPools {
    pub fn new(setup: HashMap<FixedBytes<40>, PoolIdWithDirection>) -> Self {
        AngstromPools(setup)
    }

    pub fn order_info(
        &self,
        currency_in: Address,
        currency_out: Address
    ) -> Option<PoolIdWithDirection> {
        tracing::debug!(shit=?self.0);
        self.0
            .get(&self.get_key(currency_in, currency_out))
            .copied()
    }

    #[inline(always)]
    fn get_key(&self, currency_in: Address, currency_out: Address) -> FixedBytes<40> {
        FixedBytes::concat_const(currency_in.0, currency_out.0)
    }
}
