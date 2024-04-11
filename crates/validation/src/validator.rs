use std::{
    pin::Pin,
    sync::{atomic::AtomicU64, Arc},
    task::Poll
};

use angstrom_eth::manager::EthEvent;
use futures::{Stream, StreamExt};
use futures_util::{Future, FutureExt};
use reth_provider::StateProviderFactory;
use reth_revm::db::BundleState;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::{
    bundle::{bundle_validator::BundleValidator, BundleSimRequest},
    common::lru_db::RevmLRU,
    order::{
        order_validator::OrderValidator, state::config::ValidationConfig, OrderValidationRequest
    }
};

pub enum ValidationRequest {
    Bundle(BundleSimRequest),
    Order(OrderValidationRequest)
}

#[derive(Debug, Clone)]
pub struct ValidationClient(pub UnboundedSender<ValidationRequest>);

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
    pub fn new(
        rx: UnboundedReceiver<ValidationRequest>,
        new_block_stream: Pin<Box<dyn Stream<Item = EthEvent> + Send>>,
        db: Arc<RevmLRU<DB>>,
        config: ValidationConfig,
        block_number: Arc<AtomicU64>
    ) -> Self {
        let order_validator = OrderValidator::new(db.clone(), config, block_number);
        Self { new_block_stream, db, bundle_validator: BundleValidator {}, order_validator, rx }
    }

    fn on_new_validation_request(&mut self, req: ValidationRequest) {
        match req {
            ValidationRequest::Order(order) => self.order_validator.validate_order(order),
            ValidationRequest::Bundle(bundle) => {
                todo!("bundle validation is currently not complete")
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

        self.order_validator.poll_unpin(cx)
    }
}
