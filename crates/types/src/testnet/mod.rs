use std::collections::HashMap;

use alloy_primitives::{Address, Bytes};

use crate::contract_bindings::angstrom::Angstrom::PoolKey;

#[derive(Debug, Clone)]
pub struct InitialTestnetState {
    pub angstrom_addr:     Address,
    pub pool_manager_addr: Address,
    pub state:             Option<Bytes>,
    pub pool_keys:         Vec<PoolKey>
}

impl InitialTestnetState {
    pub fn new(
        angstrom_addr: Address,
        pool_manager_addr: Address,
        state: Option<Bytes>,
        pool_keys: Vec<PoolKey>
    ) -> Self {
        Self { angstrom_addr, state, pool_manager_addr, pool_keys }
    }
}

pub struct TestnetStateOverrides {
    /// token -> user -> amount
    pub approvals: HashMap<Address, HashMap<Address, u128>>,
    /// token -> user -> amount
    pub balances:  HashMap<Address, HashMap<Address, u128>>
}
