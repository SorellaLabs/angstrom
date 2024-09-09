use alloy_primitives::B256;
#[cfg(feature = "testnet")]
use alloy_primitives::{Address, U256};
use alloy_sol_types::SolStruct;
#[cfg(feature = "testnet")]
use rand::{rngs::ThreadRng, Rng};

use crate::sol_bindings::sol::ContractBundle;
#[cfg(feature = "testnet")]
use crate::sol_bindings::sol::{SolGenericOrder, TopOfBlockOrder};

impl ContractBundle {
    pub fn get_filled_hashes(&self) -> Vec<B256> {
        self.top_of_block_orders
            .iter()
            .map(|order| order.eip712_hash_struct())
            .chain(self.orders.iter().map(|order| order.eip712_hash_struct()))
            .collect()
    }

    pub fn get_addressees_touched(&self) -> Vec<Address> {
        self.top_of_block_orders
            .iter()
            .map(|order| order.from)
            .chain(self.orders.iter().map(|order| order.from))
            .collect()
    }
}

#[cfg(feature = "testnet")]
impl ContractBundle {
    pub fn generate_random_bundles(order_count: u64) -> Self {
        let mut rng = ThreadRng::default();

        let assets = vec![Address::new(rng.gen::<[u8; 20]>())];

        let mut tob = TopOfBlockOrder::default();
        let rand_am_in: U256 = U256::from_be_bytes(rng.gen::<[u8; 32]>());
        let rand_am_out: U256 = U256::from_be_bytes(rng.gen::<[u8; 32]>());
        tob.amountIn = rand_am_in;
        tob.amountOut = rand_am_out;
        let mut generic_orders = vec![];
        for _ in 0..order_count {
            let mut default = SolGenericOrder::default();
            let specified: U256 = U256::from_be_bytes(rng.gen::<[u8; 32]>());
            default.amountSpecified = specified;
            generic_orders.push(default);
        }

        Self {
            assets,
            top_of_block_orders: vec![tob],
            orders: generic_orders,
            ..Default::default()
        }
    }
}

#[cfg(test)]
pub mod test {

    #[test]
    #[cfg(feature = "testnet")]
    pub fn test_contract_bundle_encode_decode() {
        use alloy_sol_types::SolValue;

        use crate::sol_bindings::sol::ContractBundle;

        let rand = ContractBundle::generate_random_bundles(10);
        let encoded = rand.abi_encode();
        let decoded = ContractBundle::abi_decode(&encoded, false).unwrap();
        assert_eq!(rand, decoded);
    }
}
