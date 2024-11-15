use std::{collections::HashMap, sync::Arc};

use alloy_primitives::{keccak256, Address, FixedBytes};
use angstrom_types::{
    self,
    contract_payloads::angstrom::AngstromBundle,
    orders::OrderOrigin,
    sol_bindings::{ext::RawPoolOrder, grouped_orders::AllOrders}
};
use pade::PadeEncode;
use parking_lot::Mutex;
use validation::{
    bundle::{BundleResponse, BundleValidatorHandle},
    order::{GasEstimationFuture, OrderValidationResults, OrderValidatorHandle}
};

// all keys are the signer of the order
#[derive(Debug, Clone, Default)]
pub struct MockValidator {
    pub limit_orders: Arc<Mutex<HashMap<Address, OrderValidationResults>>>,
    pub bundle_res:   Arc<Mutex<HashMap<FixedBytes<32>, BundleResponse>>>
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
    pub fn add_order(&self, signer: Address, order: OrderValidationResults) {
        inserts!(self, limit_orders, signer, order)
    }
}

//TODO: validate can be shortened using a macro

impl OrderValidatorHandle for MockValidator {
    type Order = AllOrders;

    fn new_block(
        &self,
        _: u64,
        _: Vec<alloy_primitives::B256>,
        _: Vec<Address>
    ) -> validation::order::ValidationFuture {
        Box::pin(async move { OrderValidationResults::TransitionedToBlock })
    }

    fn validate_order(
        &self,
        _origin: angstrom_types::orders::OrderOrigin,
        transaction: Self::Order
    ) -> validation::order::ValidationFuture {
        let address = transaction.from();
        let res = self
            .limit_orders
            .lock()
            .remove(&address)
            .expect("not in mock");
        Box::pin(async move { res })
    }

    fn estimate_gas(&self, order: AllOrders) -> GasEstimationFuture {
        Box::pin(async move {
            match self.validate_order(OrderOrigin::External, order).await {
                OrderValidationResults::Valid(o) => {
                    Ok((o.priority_data.gas_units, o.priority_data.gas))
                }
                OrderValidationResults::Invalid(e) => Err(format!("Invalid order: {}", e)),
                OrderValidationResults::TransitionedToBlock => {
                    Err("Order transitioned to block".to_string())
                }
            }
        })
    }
}

impl BundleValidatorHandle for MockValidator {
    async fn fetch_gas_for_bundle(&self, bundle: AngstromBundle) -> eyre::Result<BundleResponse> {
        let e = bundle.pade_encode();
        let hash = keccak256(e);
        self.bundle_res.lock().unwrap().remove(&hash)
    }
}
