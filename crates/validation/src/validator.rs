use std::{fmt::Debug, task::Poll};

use alloy::primitives::{Address, B256, U256};
use angstrom_types::contract_payloads::angstrom::{AngstromBundle, BundleGasDetails};
use futures_util::{Future, FutureExt};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::{
    bundle::BundleValidator,
    common::SharedTools,
    order::{
        OrderValidationRequest, OrderValidationResults,
        order_validator::OrderValidator,
        sim::{BOOK_GAS, TOB_GAS},
        state::{db_state_utils::StateFetchUtils, pools::PoolsTracker}
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
    },
    Nonce {
        sender:       tokio::sync::oneshot::Sender<u64>,
        user_address: Address
    },
    /// NOTE: this cancel order should already be verified
    CancelOrder {
        user:       Address,
        order_hash: B256
    },
    GasEstimation {
        sender:  tokio::sync::oneshot::Sender<eyre::Result<U256>>,
        is_book: bool,
        token_0: Address,
        token_1: Address
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
    DB: Unpin + Clone + reth_provider::BlockNumReader + revm::DatabaseRef + Send + Sync + 'static,
    Pools: PoolsTracker + Send + Sync + 'static,
    Fetch: StateFetchUtils + Send + Sync + 'static,
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
            ValidationRequest::CancelOrder { user, order_hash } => {
                self.order_validator.cancel_order(user, order_hash);
            }
            ValidationRequest::Order(order) => self.order_validator.validate_order(
                order,
                self.utils.token_pricing_snapshot(),
                &mut self.utils.thread_pool,
                self.utils.metrics.clone()
            ),
            ValidationRequest::Bundle { sender, bundle } => {
                println!("{:#?}", bundle);
                tracing::debug!("simulating bundle");
                let bn = self
                    .order_validator
                    .block_number
                    .load(std::sync::atomic::Ordering::SeqCst);
                self.bundle_validator.simulate_bundle(
                    sender,
                    bundle,
                    &self.utils.token_pricing,
                    &mut self.utils.thread_pool,
                    self.utils.metrics.clone(),
                    bn
                );
            }
            ValidationRequest::NewBlock { sender, block_number, orders, addresses } => {
                tracing::debug!("transitioning to new block");
                self.utils.metrics.eth_transition_updates(|| {
                    self.order_validator
                        .on_new_block(block_number, orders, addresses);
                });
                sender
                    .send(OrderValidationResults::TransitionedToBlock)
                    .unwrap();
            }
            ValidationRequest::Nonce { sender, user_address } => {
                let nonce = self.order_validator.fetch_nonce(user_address);
                sender.send(nonce).unwrap();
            }
            ValidationRequest::GasEstimation { sender, is_book, mut token_0, mut token_1 } => {
                if token_0 > token_1 {
                    std::mem::swap(&mut token_0, &mut token_1);
                }

                let Some(cvrt) = self
                    .utils
                    .token_pricing_ref()
                    .get_eth_conversion_price(token_0, token_1)
                else {
                    let _ = sender.send(Err(eyre::eyre!("not valid token pair")));
                    return;
                };
                // NOTE: this is currently fixed with gasprice. when we update, this will need
                // to be changed
                //
                let gas_in_wei = if is_book { BOOK_GAS } else { TOB_GAS };

                let gas_token_0 = (cvrt * U256::from(gas_in_wei)).scale_out_of_ray();
                let _ = sender.send(Ok(gas_token_0));
            }
        }
    }
}

impl<DB, Pools, Fetch> Future for Validator<DB, Pools, Fetch>
where
    DB: Unpin + Clone + 'static + revm::DatabaseRef + reth_provider::BlockNumReader + Send + Sync,
    <DB as revm::DatabaseRef>::Error: Send + Sync + Debug,
    Pools: PoolsTracker + Send + Sync + Unpin + 'static,
    Fetch: StateFetchUtils + Send + Sync + Unpin + 'static
{
    type Output = ();

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>
    ) -> std::task::Poll<Self::Output> {
        loop {
            match self.rx.poll_recv(cx) {
                Poll::Ready(Some(req)) => {
                    self.on_new_validation_request(req);
                }
                // we only check this here as we use this as the shutdown signal.
                Poll::Ready(None) => {
                    return Poll::Ready(());
                }
                _ => {
                    break;
                }
            }
        }

        self.utils.poll_unpin(cx)
    }
}
