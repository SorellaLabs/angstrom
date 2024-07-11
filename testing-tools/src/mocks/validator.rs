use std::{collections::HashMap, sync::Arc};

use alloy_primitives::Address;
use angstrom_types::{
    self,
    sol_bindings::grouped_orders::{AllOrders, OrderWithStorageData}
};
use parking_lot::Mutex;
use validation::order::OrderValidatorHandle;

// all keys are the signer of the order
#[derive(Debug, Clone, Default)]
pub struct MockValidator {
    pub limit_orders: Arc<Mutex<HashMap<Address, OrderWithStorageData<AllOrders>>>>
}

macro_rules! inserts {
    ($this:ident,$inner:ident, $signer:ident, $order:ident) => {
        if $this.$inner.lock().insert($signer, $order).is_some() {
            panic!(
                "mock validator doesn't support more than one type of order per signer, this is \
                 due to the multi threaded nature of the validator which can cause race \
                 conditions "
            );
        }
    };
}
impl MockValidator {
    pub fn add_order(&self, signer: Address, order: OrderWithStorageData<AllOrders>) {
        inserts!(self, limit_orders, signer, order)
    }
}

//TODO: validate can be shortened using a macro

impl OrderValidatorHandle for MockValidator {
    type Order = AllOrders;

    fn validate_order(
        &self,
        origin: angstrom_types::orders::OrderOrigin,
        transaction: Self::Order
    ) -> validation::order::ValidationFuture<Self::Order> {
        todo!()
    }
}
