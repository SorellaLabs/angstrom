use alloy_primitives::{Address, B256, U256};
use alloy_sol_types::SolStruct;
#[cfg(feature = "testnet")]
use rand::{rngs::ThreadRng, Rng};

use crate::sol::ContractBundle;
#[cfg(feature = "testnet")]
use crate::sol::{SolGenericOrder, SolTopOfBlockOrderEnvelope};

impl ContractBundle {
    pub fn get_filled_hashes(&self) -> Vec<B256> {
        self.top_of_block_orders
            .iter()
            .map(|order| order.eip712_hash_struct())
            .chain(self.orders.iter().map(|order| order.eip712_hash_struct()))
            .collect()
    }
}

#[cfg(feature = "testnet")]
impl ContractBundle {
    pub fn generate_random_bundles(order_count: u64) -> Self {
        let mut rng = ThreadRng::default();

        let assets = vec![Address::new(rng.gen::<[u8; 20]>())];

        let mut tob = SolTopOfBlockOrderEnvelope::default();
        let rand_am_in: U256 = rng.gen();
        let rand_am_out: U256 = rng.gen();
        tob.amountIn = rand_am_in;
        tob.amountOut = rand_am_out;
        let mut generic_orders = vec![];
        for _ in 0..order_count {
            let mut default = SolGenericOrder::default();
            let specified: U256 = rng.gen();
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
