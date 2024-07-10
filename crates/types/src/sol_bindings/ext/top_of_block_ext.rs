use alloy_primitives::FixedBytes;
use alloy_sol_types::SolStruct;

use crate::sol_bindings::sol::TopOfBlockOrder;

impl TopOfBlockOrder {
    pub fn order_hash(&self) -> FixedBytes<32> {
        self.eip712_hash_struct()
    }
}
