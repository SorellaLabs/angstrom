use std::{pin::Pin, sync::Arc, task::Poll};

use futures::{Stream, StreamExt};
use futures_util::Future;
use guard_eth::manager::EthEvent;
use reth_provider::StateProviderFactory;
use reth_revm::db::BundleState;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::{
    bundle::{bundle_validator::BundleValidator, BundleSimRequest},
    common::lru_db::RevmLRU,
    order::{order_validator::OrderValidator, OrderValidationRequest}
};

pub enum ValidationRequest {
    Bundle(BundleSimRequest),
    Order(OrderValidationRequest)
}

#[derive(Debug, Clone)]
pub struct ValidationClient(pub(crate) UnboundedSender<ValidationRequest>);

/// HeadModule that deals with all validation
#[allow(dead_code)]
pub struct Validator<DB> {
    rx:               UnboundedReceiver<ValidationRequest>,
    /// used to update state
    new_block_stream: Pin<Box<dyn Stream<Item = EthEvent> + Send>>,
    db:               Arc<RevmLRU<DB>>,

    order_validator:  OrderValidator<DB>,
    bundle_validator: BundleValidator
}

impl<DB> Validator<DB>
where
    DB: StateProviderFactory + Clone + Unpin + 'static
{
    fn new_block(&mut self, state: BundleState) {
        // TODO: update the db + deal with reseting the validation;
    }

    fn on_new_validation_request(&mut self, req: ValidationRequest) {
        match req {
            ValidationRequest::Order(order) => self.order_validator.validate_order(order),
            ValidationRequest::Bundle(bundle) => {
                todo!()
            }
        }
    }
}

impl<DB> Future for Validator<DB>
where
    DB: StateProviderFactory + Clone + Unpin + 'static
{
    type Output = ();

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>
    ) -> std::task::Poll<Self::Output> {
        while let Poll::Ready(Some(req)) = self.rx.poll_recv(cx) {
            self.on_new_validation_request(req);
        }

        Poll::Pending
    }
}
