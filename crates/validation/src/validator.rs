use std::{fmt::Debug, task::Poll};

use alloy::primitives::{Address, B256};
use angstrom_types::contract_payloads::angstrom::{AngstromBundle, BundleGasDetails};
use futures_util::{Future, FutureExt};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::{
    bundle::BundleValidator,
    common::SharedTools,
    order::{
        order_validator::OrderValidator,
        state::{db_state_utils::StateFetchUtils, pools::PoolsTracker},
        OrderValidationRequest, OrderValidationResults
    }
};

pub enum ValidationRequest {
    Order(OrderValidationRequest),
    /// does two sims, One to fetch total gas used. Second is once
    /// gas cost has be delegated to each user order. ensures we won't have a
    /// failure.
    Bundle {
        sender: tokio::sync::oneshot::Sender<eyre::Result<BundleGasDetails>>,
        bundle: AngstromBundle
    },
    NewBlock {
        sender:       tokio::sync::oneshot::Sender<OrderValidationResults>,
        block_number: u64,
        orders:       Vec<B256>,
        addresses:    Vec<Address>
    }
}

#[derive(Debug, Clone)]
pub struct ValidationClient(pub UnboundedSender<ValidationRequest>);

pub struct Validator<DB, Pools, Fetch> {
    rx:               UnboundedReceiver<ValidationRequest>,
    order_validator:  OrderValidator<DB, Pools, Fetch>,
    bundle_validator: BundleValidator<DB>,
    utils:            SharedTools
}

impl<DB, Pools, Fetch> Validator<DB, Pools, Fetch>
where
    DB: Unpin + Clone + 'static + reth_provider::BlockNumReader + revm::DatabaseRef + Send + Sync,
    Pools: PoolsTracker + Sync + 'static,
    Fetch: StateFetchUtils + Sync + 'static,
    <DB as revm::DatabaseRef>::Error: Send + Sync + Debug
{
    pub fn new(
        rx: UnboundedReceiver<ValidationRequest>,
        order_validator: OrderValidator<DB, Pools, Fetch>,
        bundle_validator: BundleValidator<DB>,
        utils: SharedTools
    ) -> Self {
        Self { order_validator, rx, utils, bundle_validator }
    }

    fn on_new_validation_request(&mut self, req: ValidationRequest) {
        match req {
            ValidationRequest::Order(order) => self.order_validator.validate_order(
                order,
                self.utils.token_pricing_snapshot(),
                &mut self.utils.thread_pool,
                &self.utils.metrics
            ),
            ValidationRequest::Bundle { sender, bundle } => {
                self.bundle_validator.simulate_bundle(
                    sender,
                    bundle,
                    &self.utils.token_pricing,
                    &mut self.utils.thread_pool
                );
            }
            ValidationRequest::NewBlock { sender, block_number, orders, addresses } => {
                self.utils.metrics.new_block(|| {
                    self.order_validator
                        .on_new_block(block_number, orders, addresses);
                });
                sender
                    .send(OrderValidationResults::TransitionedToBlock)
                    .unwrap();
            }
        }
    }
}

impl<DB, Pools, Fetch> Future for Validator<DB, Pools, Fetch>
where
    DB: Unpin + Clone + 'static + revm::DatabaseRef + reth_provider::BlockNumReader + Send + Sync,
    <DB as revm::DatabaseRef>::Error: Send + Sync + Debug,
    Pools: PoolsTracker + Sync + Unpin + 'static,
    Fetch: StateFetchUtils + Sync + Unpin + 'static
{
    type Output = ();

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>
    ) -> std::task::Poll<Self::Output> {
        while let Poll::Ready(Some(req)) = self.rx.poll_recv(cx) {
            self.on_new_validation_request(req);
        }

        self.utils.poll_unpin(cx)
    }
}
