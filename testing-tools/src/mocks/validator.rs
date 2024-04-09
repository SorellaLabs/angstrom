use std::{collections::HashMap, sync::Arc};

use alloy_primitives::Address;
use angstrom_types::{
    orders::OrderValidationOutcome,
    rpc::{
        EcRecoveredComposableLimitOrder, EcRecoveredComposableSearcherOrder, EcRecoveredLimitOrder,
        EcRecoveredSearcherOrder
    }
};
use parking_lot::Mutex;
use validation::order::OrderValidator;

// all keys are the signer of the order
#[derive(Debug, Clone)]
pub struct MockValidator {
    pub limit_orders: Arc<Mutex<HashMap<Address, OrderValidationOutcome<EcRecoveredLimitOrder>>>>,
    pub searcher_orders:
        Arc<Mutex<HashMap<Address, OrderValidationOutcome<EcRecoveredSearcherOrder>>>>,
    pub comp_limit:
        Arc<Mutex<HashMap<Address, OrderValidationOutcome<EcRecoveredComposableLimitOrder>>>>,
    pub comp_searcher:
        Arc<Mutex<HashMap<Address, OrderValidationOutcome<EcRecoveredComposableSearcherOrder>>>>
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
    pub fn add_limit_order(
        &self,
        signer: Address,
        order: OrderValidationOutcome<EcRecoveredLimitOrder>
    ) {
        inserts!(self, limit_orders, signer, order)
    }

    pub fn add_searcher_order(
        &self,
        signer: Address,
        order: OrderValidationOutcome<EcRecoveredSearcherOrder>
    ) {
        inserts!(self, searcher_orders, signer, order)
    }

    pub fn add_comp_limit(
        &self,
        signer: Address,
        order: OrderValidationOutcome<EcRecoveredComposableLimitOrder>
    ) {
        inserts!(self, comp_limit, signer, order)
    }

    pub fn add_comp_searcher(
        &self,
        signer: Address,
        order: OrderValidationOutcome<EcRecoveredComposableSearcherOrder>
    ) {
        inserts!(self, comp_searcher, signer, order)
    }
}

//TODO: validate can be shortened using a macro

impl OrderValidator for MockValidator {
    type ComposableLimitOrder = EcRecoveredComposableLimitOrder;
    type ComposableSearcherOrder = EcRecoveredComposableSearcherOrder;
    type LimitOrder = EcRecoveredLimitOrder;
    type SearcherOrder = EcRecoveredSearcherOrder;

    fn validate_order(
        &self,
        _origin: angstrom_types::orders::OrderOrigin,
        transaction: Self::LimitOrder
    ) -> validation::order::ValidationFuture<Self::LimitOrder> {
        let key = transaction.signer;
        if let Some(res) = self.limit_orders.lock().remove(&key) {
            return Box::pin(async move { res })
        }
        Box::pin(async move {
            OrderValidationOutcome::Invalid(
                transaction,
                angstrom_types::orders::ValidationError::BadSigner
            )
        })
    }

    fn validate_searcher_order(
        &self,
        _origin: angstrom_types::orders::OrderOrigin,
        transaction: Self::SearcherOrder
    ) -> validation::order::ValidationFuture<Self::SearcherOrder> {
        let key = transaction.signer;
        if let Some(res) = self.searcher_orders.lock().remove(&key) {
            return Box::pin(async move { res })
        }
        Box::pin(async move {
            OrderValidationOutcome::Invalid(
                transaction,
                angstrom_types::orders::ValidationError::BadSigner
            )
        })
    }

    fn validate_composable_order(
        &self,
        _origin: angstrom_types::orders::OrderOrigin,
        transaction: Self::ComposableLimitOrder
    ) -> validation::order::ValidationFuture<Self::ComposableLimitOrder> {
        let key = transaction.signer;
        if let Some(res) = self.comp_limit.lock().remove(&key) {
            return Box::pin(async move { res })
        }
        Box::pin(async move {
            OrderValidationOutcome::Invalid(
                transaction,
                angstrom_types::orders::ValidationError::BadSigner
            )
        })
    }

    fn validate_composable_searcher_order(
        &self,
        _origin: angstrom_types::orders::OrderOrigin,
        transaction: Self::ComposableSearcherOrder
    ) -> validation::order::ValidationFuture<Self::ComposableSearcherOrder> {
        let key = transaction.signer;
        if let Some(res) = self.comp_searcher.lock().remove(&key) {
            return Box::pin(async move { res })
        }
        Box::pin(async move {
            OrderValidationOutcome::Invalid(
                transaction,
                angstrom_types::orders::ValidationError::BadSigner
            )
        })
    }
}
