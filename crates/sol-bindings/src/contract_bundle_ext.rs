use alloy_primitives::B256;
use alloy_sol_types::SolStruct;

use crate::sol::ContractBundle;

impl ContractBundle {
    pub fn get_filled_hashes(&self) -> Vec<B256> {
        self.top_of_block_orders
            .iter()
            .map(|order| order.eip712_type_hash())
            .chain(self.orders.iter().map(|order| order.eip712_type_hash()))
            .collect()
    }
}
