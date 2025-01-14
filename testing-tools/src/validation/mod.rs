use std::{
    future::{poll_fn, Future},
    pin::Pin,
    sync::{atomic::AtomicU64, Arc},
    task::Poll,
    time::Duration
};

use alloy_primitives::{Address, U256};
use angstrom_types::pair_with_price::PairsWithPrice;
use futures::{FutureExt, Stream};
use reth_provider::BlockNumReader;
use tokio::sync::mpsc::UnboundedReceiver;
use uniswap_v4::uniswap::pool_manager::SyncedUniswapPools;
use validation::{
    bundle::BundleValidator,
    common::{
        db::BlockStateProviderFactory, key_split_threadpool::KeySplitThreadpool, SharedTools,
        TokenPriceGenerator
    },
    order::{
        order_validator::OrderValidator,
        sim::SimValidation,
        state::{
            db_state_utils::{nonces::Nonces, AutoMaxFetchUtils},
            pools::AngstromPoolsTracker
        }
    },
    validator::{ValidationClient, ValidationRequest, Validator}
};

type ValidatorOperation<DB, T> =
    dyn FnOnce(
        TestOrderValidator<DB>,
        T
    ) -> Pin<Box<dyn Future<Output = (TestOrderValidator<DB>, T)> + Send>>;

pub struct TestOrderValidator<DB>
where
    DB: BlockStateProviderFactory + revm::DatabaseRef + Clone + Unpin + 'static
{
    /// allows us to set values to ensure
    pub db:         Arc<DB>,
    pub node_id:    u64,
    pub client:     ValidationClient,
    pub underlying: Validator<DB, AngstromPoolsTracker, AutoMaxFetchUtils>
}

impl<DB> TestOrderValidator<DB>
where
    DB: BlockStateProviderFactory + Clone + Unpin + revm::DatabaseRef + BlockNumReader + 'static,
    <DB as revm::DatabaseRef>::Error: Send + Sync + std::fmt::Debug
{
    pub async fn new(
        db: DB,
        validation_client: ValidationClient,
        validator_rx: UnboundedReceiver<ValidationRequest>,
        angstrom_address: Address,
        node_address: Address,
        uniswap_pools: SyncedUniswapPools,
        token_conversion: TokenPriceGenerator,
        token_updates: Pin<Box<dyn Stream<Item = Vec<PairsWithPrice>> + Send + Sync + 'static>>,
        pool_storage: AngstromPoolsTracker,
        node_id: u64
    ) -> eyre::Result<Self> {
        let current_block = Arc::new(AtomicU64::new(BlockNumReader::best_block_number(&db)?));
        let db = Arc::new(db);

        let fetch = AutoMaxFetchUtils;

        let handle = tokio::runtime::Handle::current();
        let thread_pool = KeySplitThreadpool::new(handle, 3);
        let sim = SimValidation::new(db.clone(), angstrom_address, node_address);

        let order_validator =
            OrderValidator::new(sim, current_block, pool_storage, fetch, uniswap_pools).await;

        let bundle_validator = BundleValidator::new(db.clone(), angstrom_address, node_address);
        let shared_utils = SharedTools::new(token_conversion, token_updates, thread_pool);

        let val = Validator::new(validator_rx, order_validator, bundle_validator, shared_utils);

        Ok(Self { db, client: validation_client, underlying: val, node_id })
    }

    pub async fn poll_for(&mut self, duration: Duration) {
        let _ = tokio::time::timeout(
            duration,
            poll_fn(|cx| {
                if self.underlying.poll_unpin(cx).is_ready() {
                    return Poll::Ready(())
                }
                cx.waker().wake_by_ref();
                Poll::Pending
            })
        )
        .await;
    }

    pub fn generate_nonce_slot(&self, user: Address, nonce: u64) -> U256 {
        Nonces::new(Address::default())
            .get_nonce_word_slot(user, nonce)
            .into()
    }
}

impl<DB> Future for TestOrderValidator<DB>
where
    DB: BlockStateProviderFactory + Clone + Unpin + revm::DatabaseRef + BlockNumReader + 'static,
    <DB as revm::DatabaseRef>::Error: Send + Sync + std::fmt::Debug + Unpin
{
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let span = tracing::span!(tracing::Level::ERROR, "validator", self.node_id);
        let e = span.enter();
        let r = self.underlying.poll_unpin(cx);
        drop(e);
        r
    }
}

pub struct OrderValidatorChain<DB, T>
where
    DB: BlockStateProviderFactory + Clone + Unpin + 'static + revm::DatabaseRef,
    T: 'static
{
    validator:     TestOrderValidator<DB>,
    state:         T,
    operations:    Vec<Box<ValidatorOperation<DB, T>>>,
    poll_duration: Duration
}

impl<DB, T> OrderValidatorChain<DB, T>
where
    DB: BlockStateProviderFactory + Clone + Unpin + 'static + revm::DatabaseRef + BlockNumReader,
    T: 'static,
    <DB as revm::DatabaseRef>::Error: Send + Sync + std::fmt::Debug
{
    pub fn new(validator: TestOrderValidator<DB>, poll_duration: Duration, state: T) -> Self {
        Self { poll_duration, validator, operations: vec![], state }
    }

    pub fn add_operation<F>(mut self, op: F) -> Self
    where
        F: FnOnce(
                TestOrderValidator<DB>,
                T
            ) -> Pin<Box<dyn Future<Output = (TestOrderValidator<DB>, T)> + Send>>
            + 'static
    {
        self.operations.push(Box::new(op));
        self
    }

    pub async fn execute_all_operations(self) {
        let (mut pool, mut state) = (self.validator, self.state);

        for op in self.operations {
            pool.poll_for(self.poll_duration).await;

            // because we insta await. this is safe. so we can tell the rust analyzer to
            // stop being annoying
            let (r_pool, r_state) = (op)(pool, state).await;
            pool = r_pool;
            state = r_state;
        }
    }
}
