use std::{
    pin::Pin,
    sync::{atomic::AtomicU64, Arc},
    task::Poll
};

use alloy::primitives::{Address, B256};
use angstrom_utils::key_split_threadpool::KeySplitThreadpool;
use futures_util::{Future, FutureExt};
use tokio::{
    runtime::Handle,
    sync::mpsc::{UnboundedReceiver, UnboundedSender}
};

use crate::{
    common::db::BlockStateProviderFactory,
    order::{
        order_validator::OrderValidator,
        sim::SimValidation,
        state::{account::user::UserAddress, db_state_utils::StateFetchUtils, pools::PoolsTracker},
        OrderValidationRequest, OrderValidationResults
    }
};

pub enum ValidationRequest {
    Order(OrderValidationRequest),
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
    rx:              UnboundedReceiver<ValidationRequest>,
    order_validator: OrderValidator<DB, Pools, Fetch>
}

impl<DB, Pools, Fetch> Validator<DB, Pools, Fetch>
where
    DB: BlockStateProviderFactory + Unpin + Clone + 'static + revm::DatabaseRef,
    Pools: PoolsTracker + Sync + 'static,
    Fetch: StateFetchUtils + Sync + 'static
{
    pub fn new(
        rx: UnboundedReceiver<ValidationRequest>,
        order_validator: OrderValidator<DB, Pools, Fetch>
    ) -> Self {
        Self { order_validator, rx }
    }

    fn on_new_validation_request(&mut self, req: ValidationRequest) {
        match req {
            ValidationRequest::Order(order) => self.order_validator.validate_order(order),
            ValidationRequest::NewBlock { sender, block_number, orders, addresses } => {
                self.order_validator
                    .on_new_block(block_number, orders, addresses);
                sender
                    .send(OrderValidationResults::TransitionedToBlock)
                    .unwrap();
            }
        }
    }
}

impl<DB, Pools, Fetch> Future for Validator<DB, Pools, Fetch>
where
    DB: BlockStateProviderFactory + Unpin + Clone + 'static + revm::DatabaseRef,
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

        self.order_validator.poll_unpin(cx)
    }
}
