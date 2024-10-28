use std::{
    future::{poll_fn, Future},
    path::Path,
    pin::Pin,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc
    },
    task::Poll,
    time::Duration
};

use alloy_primitives::{Address, U256};
use angstrom_utils::key_split_threadpool::KeySplitThreadpool;
use futures::FutureExt;
use matching_engine::cfmm::uniswap::{
    pool_manager::UniswapPoolManager,
    pool_providers::canonical_state_adapter::CanonicalStateAdapter
};
use reth_provider::CanonStateNotification;
use tokio::sync::mpsc::unbounded_channel;
use validation::{
    order::{
        order_validator::OrderValidator,
        sim::SimValidation,
        state::{
            config::{load_data_fetcher_config, load_validation_config, ValidationConfig},
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
    DB: StateProviderFactory + revm::DatabaseRef + Clone + Unpin + 'static
> {
    /// allows us to set values to ensure
    pub db:         Arc<DB>,
    pub config:     ValidationConfig,
    pub client:     ValidationClient,
    pub underlying: Validator<DB, AngstromPoolsTracker, FetchUtils<DB>, CanonicalStateAdapter>
}

impl<DB: StateProviderFactory + Clone + Unpin + 'static + revm::DatabaseRef> TestOrderValidator<DB>
where
    <DB as revm::DatabaseRef>::Error: Send + Sync + std::fmt::Debug
{
    pub fn new(db: DB) -> Self {
        let (tx, rx) = unbounded_channel();
        let config_path = Path::new("./state_config.toml");
        let fetch_config = load_data_fetcher_config(config_path).unwrap();
        let validation_config = load_validation_config(config_path).unwrap();
        tracing::debug!(?fetch_config, ?validation_config);
        let current_block = Arc::new(AtomicU64::new(db.best_block_number().unwrap()));
        let db = Arc::new(db);

        let fetch = FetchUtils::new(fetch_config.clone(), db.clone());
        let pools = AngstromPoolsTracker::new(validation_config.clone());

        let handle = tokio::runtime::Handle::current();
        let thread_pool =
            KeySplitThreadpool::new(handle, validation_config.max_validation_per_user);
        let sim = SimValidation::new(db.clone());
        let (_, state_notification) =
            tokio::sync::broadcast::channel::<CanonStateNotification>(100);

        let pool_manager = UniswapPoolManager::new(
            vec![],
            current_block.load(Ordering::SeqCst),
            100,
            Arc::new(CanonicalStateAdapter::new(state_notification))
        );

        let order_validator =
            OrderValidator::new(sim, current_block, pools, fetch, pool_manager, thread_pool);

        let val = Validator::new(rx, order_validator);
        let client = ValidationClient(tx);

        Self { db, client, underlying: val, config: validation_config }
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
        Nonces.get_nonce_word_slot(user, nonce).into()
    }
}

pub struct OrderValidatorChain<
    DB: StateProviderFactory + Clone + Unpin + 'static + revm::DatabaseRef,
    T: 'static
> {
    validator:     TestOrderValidator<DB>,
    state:         T,
    operations:    Vec<Box<ValidatorOperation<DB, T>>>,
    poll_duration: Duration
}

impl<DB: StateProviderFactory + Clone + Unpin + 'static + revm::DatabaseRef, T: 'static>
    OrderValidatorChain<DB, T>
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
