use std::{collections::HashSet, sync::Arc};

use alloy_primitives::{hex, Address, FixedBytes};
use reth_provider::StateProviderFactory;

use crate::common::lru_db::RevmLRU;

const ANGSTROM_ADDRESS: Address =
    Address(FixedBytes(hex!("680A025Da7b1be2c204D7745e809919bCE074026")));

pub struct AngstromTokens {
    current_tokens: HashSet<Address>
}

impl AngstromTokens {
    pub fn new<DB>(db: DB) -> Self {
        Self { current_tokens: HashSet::default() }
    }

    pub fn get_all_tokens(&self) -> &HashSet<Address> {
        &self.current_tokens
    }

    pub fn check_for_new_tokens<DB: StateProviderFactory>(
        &mut self,
        db: Arc<RevmLRU<DB>>
    ) -> HashSet<Address> {
        todo!()
    }
}
