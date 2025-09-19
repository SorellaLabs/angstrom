use std::{fmt::Debug, sync::Arc, task::Poll};

use alloy::primitives::{Address, B256, U256};
use angstrom_types::{
    contract_payloads::angstrom::{AngstromBundle, BundleGasDetails},
    reth_db_wrapper::SetBlock
};
use futures_util::{Future, FutureExt};
use telemetry_recorder::telemetry_event;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::{
    bundle::BundleValidator,
    common::SharedTools,
    order::{
        OrderValidationRequest, OrderValidationResults,
        order_validator::OrderValidator,
        sim::{BOOK_GAS, BOOK_GAS_INTERNAL, TOB_GAS, TOB_GAS_INTERNAL},
        state::{
            account::{UserAccountProcessor, user::UserAccounts},
            db_state_utils::StateFetchUtils,
            pools::PoolsTracker
        }
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
        sender:      tokio::sync::oneshot::Sender<eyre::Result<(U256, u64)>>,
        is_book:     bool,
        is_internal: bool,
        token_0:     Address,
        token_1:     Address
    }
}

#[derive(Debug, Clone)]
pub struct ValidationClient(pub UnboundedSender<ValidationRequest>);

pub struct Validator<DB, Pools, Fetch> {
    rx:               UnboundedReceiver<ValidationRequest>,
    order_validator:  OrderValidator<DB, Pools, Fetch>,
    bundle_validator: BundleValidator<DB>,
    utils:            SharedTools,
    db:               Arc<DB>
}

impl<DB, Pools, Fetch> Validator<DB, Pools, Fetch>
where
    DB: Unpin
        + Clone
        + reth_provider::BlockNumReader
        + revm::DatabaseRef
        + Send
        + Sync
        + 'static
        + SetBlock,
    Pools: PoolsTracker + Send + Sync + 'static,
    Fetch: StateFetchUtils + Send + Sync + 'static,
    <DB as revm::DatabaseRef>::Error: Send + Sync + Debug
{
    pub fn new(
        rx: UnboundedReceiver<ValidationRequest>,
        order_validator: OrderValidator<DB, Pools, Fetch>,
        bundle_validator: BundleValidator<DB>,
        utils: SharedTools,
        db: Arc<DB>
    ) -> Self {
        Self { order_validator, rx, utils, bundle_validator, db }
    }

    pub fn set_user_account(&mut self, account: UserAccounts) {
        let fetch_clone = self
            .order_validator
            .state
            .user_account_tracker
            .fetch_utils
            .clone();
        let new_tracker = Arc::new(UserAccountProcessor::new_with_accounts(fetch_clone, account));

        self.order_validator.state.user_account_tracker = new_tracker;
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
                tracing::debug!("simulating bundle");
                let bn = self
                    .order_validator
                    .block_number
                    .load(std::sync::atomic::Ordering::SeqCst);
                self.bundle_validator.simulate_bundle(
                    sender,
                    bundle,
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

                self.db.set_block(block_number);

                let gas_updates = self.utils.token_pricing_ref().generate_gas_updates();
                sender
                    .send(OrderValidationResults::TransitionedToBlock(gas_updates))
                    .unwrap();
                telemetry_event!(block_number, self.utils.token_pricing_ref().to_snapshot());
            }
            ValidationRequest::Nonce { sender, user_address } => {
                let nonce = self.order_validator.fetch_nonce(user_address);
                let _ = sender.send(nonce);
            }
            ValidationRequest::GasEstimation {
                sender,
                is_book,
                is_internal,
                mut token_0,
                mut token_1
            } => {
                if token_0 > token_1 {
                    std::mem::swap(&mut token_0, &mut token_1);
                }

                let gas_in_wei = match (is_book, is_internal) {
                    (true, true) => BOOK_GAS_INTERNAL,
                    (false, true) => TOB_GAS_INTERNAL,
                    (true, false) => BOOK_GAS,
                    (false, false) => TOB_GAS
                };

                let Some(mut amount) = self
                    .utils
                    .token_pricing_ref()
                    .get_eth_conversion_price(token_0, token_1, gas_in_wei)
                else {
                    let _ = sender.send(Err(eyre::eyre!("not valid token pair")));
                    return;
                };
                let block = self.utils.token_pricing_ref().current_block();

                if amount == 0 {
                    amount += 1;
                }

                let _ = sender.send(Ok((U256::from(amount), block)));
            }
        }
    }

    pub fn token_price_generator(&self) -> crate::TokenPriceGenerator {
        self.utils.token_pricing.clone()
    }
}

impl<DB, Pools, Fetch> Future for Validator<DB, Pools, Fetch>
where
    DB: Unpin
        + Clone
        + 'static
        + revm::DatabaseRef
        + reth_provider::BlockNumReader
        + Send
        + Sync
        + SetBlock,
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
