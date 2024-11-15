use std::{
    future::{poll_fn, Future},
    pin::Pin,
    sync::{atomic::AtomicU64, Arc},
    task::Poll,
    time::Duration
};

use alloy_primitives::{Address, U256};
use angstrom_types::{
    contract_payloads::angstrom::AngstromPoolConfigStore, pair_with_price::PairsWithPrice
};
use angstrom_utils::key_split_threadpool::KeySplitThreadpool;
use futures::{FutureExt, Stream};
use reth_provider::BlockNumReader;
use tokio::sync::mpsc::unbounded_channel;
use uniswap_v4::uniswap::pool_manager::SyncedUniswapPools;
use validation::{
    bundle::BundleValidator,
    common::{db::BlockStateProviderFactory, SharedTools, TokenPriceGenerator},
    order::{
        order_validator::OrderValidator,
        sim::SimValidation,
        state::{
            db_state_utils::{nonces::Nonces, FetchUtils},
            pools::AngstromPoolsTracker
        }
    },
    validator::{ValidationClient, Validator}
};

type ValidatorOperation<DB, T> =
    dyn FnOnce(
        TestOrderValidator<DB>,
        T
    ) -> Pin<Box<dyn Future<Output = (TestOrderValidator<DB>, T)>>>;

pub struct TestOrderValidator<
    DB: BlockStateProviderFactory + revm::DatabaseRef + Clone + Unpin + 'static
> {
    /// allows us to set values to ensure
    pub db:         Arc<DB>,
    pub client:     ValidationClient,
    pub underlying: Validator<DB, AngstromPoolsTracker, FetchUtils<DB>>
}

impl<
        DB: BlockStateProviderFactory + Clone + Unpin + revm::DatabaseRef + BlockNumReader + 'static
    > TestOrderValidator<DB>
where
    <DB as revm::DatabaseRef>::Error: Send + Sync + std::fmt::Debug
{
    pub async fn new(
        db: DB,
        angstrom_address: Address,
        node_address: Address,
        uniswap_pools: SyncedUniswapPools,
        token_conversion: TokenPriceGenerator,
        token_updates: Pin<Box<dyn Stream<Item = Vec<PairsWithPrice>> + 'static>>,
        pool_store: Arc<AngstromPoolConfigStore>
    ) -> Self {
        let (tx, rx) = unbounded_channel();

        let current_block =
            Arc::new(AtomicU64::new(BlockNumReader::best_block_number(&db).unwrap()));
        let db = Arc::new(db);

        let fetch = FetchUtils::new(Address::default(), db.clone());
        let pools = AngstromPoolsTracker::new(angstrom_address, pool_store);

        let handle = tokio::runtime::Handle::current();
        let thread_pool = KeySplitThreadpool::new(handle, 3);
        let sim = SimValidation::new(db.clone(), None);

        let order_validator =
            OrderValidator::new(sim, current_block, pools, fetch, uniswap_pools).await;

        let bundle_validator = BundleValidator::new(db.clone(), angstrom_address, node_address);
        let shared_utils = SharedTools::new(token_conversion, token_updates, thread_pool);

        let val = Validator::new(rx, order_validator, bundle_validator, shared_utils);
        let client = ValidationClient(tx);

        Self { db, client, underlying: val }
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

pub struct OrderValidatorChain<
    DB: BlockStateProviderFactory + Clone + Unpin + 'static + revm::DatabaseRef,
    T: 'static
> {
    validator:     TestOrderValidator<DB>,
    state:         T,
    operations:    Vec<Box<ValidatorOperation<DB, T>>>,
    poll_duration: Duration
}

impl<
        DB: BlockStateProviderFactory + Clone + Unpin + 'static + revm::DatabaseRef + BlockNumReader,
        T: 'static
    > OrderValidatorChain<DB, T>
where
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
            ) -> Pin<Box<dyn Future<Output = (TestOrderValidator<DB>, T)>>>
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
