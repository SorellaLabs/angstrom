use std::collections::HashMap;

use alloy::sol_types::SolValue;
use alloy_primitives::{keccak256, Address, Bytes, B256, U256};

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

impl TestnetStateOverrides {
    pub fn into_slots_with_overrides(
        self,
        angstrom_addr: Address
    ) -> impl Iterator<Item = (Address, B256, U256)> + 'static {
        self.approvals
            .into_iter()
            .flat_map(move |(token, i)| {
                i.into_iter().map(move |(user, amount)| {
                    let slot =
                        keccak256((angstrom_addr, keccak256((user, 2).abi_encode())).abi_encode());
                    (token, slot, U256::from(amount))
                })
            })
            .chain(self.balances.into_iter().flat_map(move |(token, i)| {
                i.into_iter().map(move |(user, qty)| {
                    let slot = keccak256((user, 1).abi_encode());
                    (token, slot, U256::from(qty))
                })
            }))
    }
}
