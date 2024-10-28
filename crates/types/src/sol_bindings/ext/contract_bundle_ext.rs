use alloy::{primitives::B256, sol_types::SolStruct};
use alloy_primitives::Address;

use crate::sol_bindings::sol::ContractBundle;

impl ContractBundle {
    pub fn get_filled_hashes(&self) -> Vec<B256> {
        self.top_of_block_orders
            .iter()
            .map(|order| order.eip712_hash_struct())
            .chain(self.orders.iter().map(|order| order.eip712_hash_struct()))
            .collect()
    }

    pub fn get_addresses_touched(&self) -> Vec<Address> {
        self.top_of_block_orders
            .iter()
            .map(|order| order.from)
            .chain(self.orders.iter().map(|order| order.from))
            .collect()
    }
}

// #[cfg(test)]
// pub mod test {

//     #[test]
//     #[cfg(feature = "testnet")]
//     pub fn test_contract_bundle_encode_decode() {
//         use alloy::sol_types::SolValue;

//         use crate::sol_bindings::sol::ContractBundle;

//         let rand = ContractBundle::generate_random_bundles(10);
//         let encoded = rand.abi_encode();
//         let decoded = ContractBundle::abi_decode(&encoded, false).unwrap();
//         assert_eq!(rand, decoded);
//     }
// }
