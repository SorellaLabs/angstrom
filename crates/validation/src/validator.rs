use std::{fmt::Debug, sync::Arc, task::Poll};

use alloy::{
    eips::BlockNumHash,
    primitives::{Address, B256, U256}
};
use angstrom_types::{
    contract_payloads::angstrom::{AngstromBundle, BundleGasDetails},
    reth_db_wrapper::AtBlock
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
        sim::{
            BOOK_GAS, BOOK_GAS_INTERNAL, SWITCH_WEI, TOB_GAS_INTERNAL_NORMAL, TOB_GAS_INTERNAL_SUB,
            TOB_GAS_NORMAL, TOB_GAS_SUB
        },
        state::{
            account::{UserAccountProcessor, user::UserAccounts},
            db_state_utils::{Repoint, StateFetchUtils},
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
        sender:      tokio::sync::oneshot::Sender<eyre::Result<BundleGasDetails>>,
        bundle:      AngstromBundle,
        /// The parent H to simulate against. Named by the caller, and carried
        /// back on the result.
        parent_hash: B256
    },
    NewBlock {
        sender:    tokio::sync::oneshot::Sender<OrderValidationResults>,
        /// The head, named by the notification rather than looked up, so a
        /// same-height reorg is followed to the branch it names.
        block:     BlockNumHash,
        orders:    Vec<B256>,
        addresses: Vec<Address>
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
    /// The state source order validation takes a fresh view of on each block.
    db:               Arc<DB>
}

impl<DB, Pools, Fetch> Validator<DB, Pools, Fetch>
where
    DB: Unpin
        + Clone
        + reth_provider::BlockNumReader
        + reth_provider::HeaderProvider
        + reth_provider::ChainSpecProvider<ChainSpec: reth_chainspec::EthereumHardforks>
        + revm::DatabaseRef
        + Send
        + Sync
        + 'static
        + AtBlock,
    Pools: PoolsTracker + Send + Sync + 'static,
    Fetch: StateFetchUtils + Repoint<DB> + Send + Sync + 'static,
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
            ValidationRequest::Bundle { sender, bundle, parent_hash } => {
                tracing::debug!(?parent_hash, "simulating bundle");
                self.bundle_validator.simulate_bundle(
                    sender,
                    bundle,
                    parent_hash,
                    &mut self.utils.thread_pool,
                    self.utils.metrics.clone()
                );
            }
            ValidationRequest::NewBlock { sender, block, orders, addresses } => {
                tracing::debug!("transitioning to new block");
                // Order validation follows the head through a fresh view per block,
                // never by moving a view a queued simulation may be reading through.
                self.order_validator
                    .repoint(Arc::new(self.db.at_block(block)));
                self.utils.metrics.eth_transition_updates(|| {
                    self.order_validator
                        .on_new_block(block.number, orders, addresses);
                });

                let gas_updates = self.utils.token_pricing_ref().generate_gas_updates();
                sender
                    .send(OrderValidationResults::TransitionedToBlock(gas_updates))
                    .unwrap();
                telemetry_event!(block.number, self.utils.token_pricing_ref().to_snapshot());
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

                let wei_price = self.utils.token_pricing_ref().base_wei;

                let (internal, normal) = if wei_price > SWITCH_WEI {
                    (TOB_GAS_INTERNAL_NORMAL, TOB_GAS_NORMAL)
                } else {
                    (TOB_GAS_INTERNAL_SUB, TOB_GAS_SUB)
                };

                let gas_in_wei = match (is_book, is_internal) {
                    (true, true) => BOOK_GAS_INTERNAL,
                    (true, false) => BOOK_GAS,
                    (false, true) => internal,
                    (false, false) => normal
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
        + reth_provider::HeaderProvider
        + reth_provider::ChainSpecProvider<ChainSpec: reth_chainspec::EthereumHardforks>
        + Send
        + Sync
        + AtBlock,
    <DB as revm::DatabaseRef>::Error: Send + Sync + Debug,
    Pools: PoolsTracker + Send + Sync + Unpin + 'static,
    Fetch: StateFetchUtils + Repoint<DB> + Send + Sync + Unpin + 'static
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

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::atomic::AtomicU64, task::Poll};

    use angstrom_types::primitive::AngstromAddressConfig;
    use tokio::{runtime::Handle, sync::oneshot};
    use uniswap_v4::uniswap::pool_manager::SyncedUniswapPools;

    use super::*;
    use crate::{
        bundle::tests::{FakeDb, PARENT, PARENT_NUMBER},
        common::{TokenPriceGenerator, key_split_threadpool::KeySplitThreadpool},
        order::{
            sim::SimValidation,
            state::{db_state_utils::FetchUtils, pools::AngstromPoolsTracker}
        }
    };

    type TestValidator = Validator<FakeDb, AngstromPoolsTracker, FetchUtils<FakeDb>>;

    /// A validator over `db`, plus the request sender that keeps it alive: a
    /// closed request channel is its shutdown signal.
    async fn validator(db: FakeDb) -> (TestValidator, UnboundedSender<ValidationRequest>) {
        AngstromAddressConfig::INTERNAL_TESTNET.try_init();
        let angstrom = Address::repeat_byte(0xaa);
        let node = Address::repeat_byte(0xbb);
        let db = Arc::new(db);
        let pools = SyncedUniswapPools::new(Default::default(), tokio::sync::mpsc::channel(1).0);

        let order_validator = OrderValidator::new(
            SimValidation::new(db.clone(), angstrom, node),
            Arc::new(AtomicU64::new(PARENT_NUMBER)),
            AngstromPoolsTracker::new(angstrom, Default::default()),
            FetchUtils::new(angstrom, db.clone()),
            pools.clone()
        )
        .await;
        let utils = SharedTools::new(
            TokenPriceGenerator::from_snapshot(pools, HashMap::new(), Address::ZERO, 0),
            Box::pin(futures::stream::pending()),
            KeySplitThreadpool::new(Handle::current(), 1)
        );
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let validator = Validator::new(
            rx,
            order_validator,
            BundleValidator::new(db.clone(), angstrom, node),
            utils,
            db
        );

        (validator, tx)
    }

    /// The block order validation's state reads are pinned to.
    fn order_view(validator: &TestValidator) -> Option<BlockNumHash> {
        validator
            .order_validator
            .state
            .user_account_tracker
            .fetch_utils
            .db
            .pinned()
    }

    fn new_block(
        validator: &mut TestValidator,
        block: BlockNumHash
    ) -> oneshot::Receiver<OrderValidationResults> {
        let (sender, rx) = oneshot::channel();
        validator.on_new_validation_request(ValidationRequest::NewBlock {
            sender,
            block,
            orders: vec![],
            addresses: vec![]
        });
        rx
    }

    /// Polls `validator` until `rx` resolves. A queued simulation only runs
    /// while the validator's thread pool is polled.
    async fn drive<T>(validator: &mut TestValidator, rx: oneshot::Receiver<T>) -> T {
        tokio::select! {
            res = rx => res.unwrap(),
            _ = futures::future::poll_fn(|cx| {
                let _ = validator.poll_unpin(cx);
                Poll::<()>::Pending
            }) => unreachable!()
        }
    }

    #[tokio::test]
    async fn a_new_block_and_a_queued_simulation_cannot_move_each_other() {
        let db = FakeDb::knowing(PARENT_NUMBER);
        let (mut validator, _keep_open) = validator(db.clone()).await;
        let parent = BlockNumHash::new(PARENT_NUMBER, PARENT);

        // Queue a simulation against the parent. It runs only once polled.
        let (sender, rx) = oneshot::channel();
        validator.on_new_validation_request(ValidationRequest::Bundle {
            sender,
            bundle: AngstromBundle::new(vec![], vec![], vec![], vec![], vec![]),
            parent_hash: PARENT
        });

        // Move the head under it before it has run.
        let head = BlockNumHash::new(PARENT_NUMBER + 1, B256::repeat_byte(0xb2));
        new_block(&mut validator, head).await.unwrap();
        assert_eq!(order_view(&validator), Some(head));

        // It still resolved the parent it was handed, and every read it made went
        // through a view of that parent — not the head that arrived meanwhile.
        let details = drive(&mut validator, rx).await.unwrap();
        assert_eq!(details.parent(), parent);
        let reads = db.reads();
        assert!(!reads.is_empty(), "the simulation read nothing");
        assert!(reads.iter().all(|read| *read == Some(parent)), "{reads:?}");

        // And running it moved nothing the other way: order validation still reads
        // the head.
        assert_eq!(order_view(&validator), Some(head));
    }

    #[tokio::test]
    async fn a_transition_pins_order_validation_to_the_block_it_names() {
        let db = FakeDb::knowing(PARENT_NUMBER);
        let (mut validator, _keep_open) = validator(db.clone()).await;
        let head = BlockNumHash::new(PARENT_NUMBER + 1, B256::repeat_byte(0xb2));
        let other_branch = BlockNumHash::new(head.number, B256::repeat_byte(0xc3));

        new_block(&mut validator, head).await.unwrap();
        assert_eq!(order_view(&validator), Some(head));

        // Same height, different branch: followed, not mistaken for the block it
        // already has.
        new_block(&mut validator, other_branch).await.unwrap();
        assert_eq!(order_view(&validator), Some(other_branch));

        // Neither transition looked anything up, so no unavailable state can stop
        // one.
        assert_eq!(db.reads(), vec![]);
    }
}
