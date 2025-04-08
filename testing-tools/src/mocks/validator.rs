use std::{collections::HashMap, sync::Arc};

use alloy_primitives::{Address, FixedBytes, U256, keccak256};
use angstrom_types::{
    self,
    contract_payloads::angstrom::{AngstromBundle, BundleGasDetails},
    sol_bindings::{ext::RawPoolOrder, grouped_orders::AllOrders}
};
use eyre::OptionExt;
use futures::future;
use pade::PadeEncode;
use parking_lot::Mutex;
use validation::{
    bundle::BundleValidatorHandle,
    order::{GasEstimationFuture, OrderValidationResults, OrderValidatorHandle}
};

// all keys are the signer of the order
#[derive(Debug, Clone, Default)]
pub struct MockValidator {
    pub limit_orders: Arc<Mutex<HashMap<Address, OrderValidationResults>>>,
    pub bundle_res:   Arc<Mutex<HashMap<FixedBytes<32>, BundleGasDetails>>>
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

impl OrderValidatorHandle for MockValidator {
    type Order = AllOrders;

    fn cancel_order(&self, _: Address, _: alloy_primitives::B256) {}

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

    fn estimate_gas(
        &self,
        _is_book: bool,
        _token_0: Address,
        _token_1: Address
    ) -> GasEstimationFuture {
        Box::pin(future::ready(Ok(U256::from(250_000u64))))
    }

    fn valid_nonce_for_user(&self, _: Address) -> validation::order::NonceFuture {
        Box::pin(async move { 10 })
    }
}

impl BundleValidatorHandle for MockValidator {
    async fn fetch_gas_for_bundle(&self, bundle: AngstromBundle) -> eyre::Result<BundleGasDetails> {
        let e = bundle.pade_encode();
        let hash = keccak256(e);

        self.bundle_res
            .lock()
            .remove(&hash)
            .ok_or_eyre("mock validator could't find bundle")
    }
}
