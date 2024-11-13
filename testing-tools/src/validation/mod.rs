use std::{
    future::{poll_fn, Future},
    path::Path,
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
use matching_engine::cfmm::uniswap::pool_manager::SyncedUniswapPools;
use reth_provider::BlockNumReader;
use tokio::sync::mpsc::unbounded_channel;
use validation::{
    common::db::BlockStateProviderFactory,
    order::{
        order_validator::OrderValidator,
        sim::SimValidation,
        state::{
            config::{load_validation_config, ValidationConfig},
            db_state_utils::{nonces::Nonces, FetchUtils},
            pools::AngstromPoolsTracker,
            token_pricing::TokenPriceGenerator
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
    pub config:     ValidationConfig,
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
        uniswap_pools: SyncedUniswapPools,
        token_conversion: TokenPriceGenerator,
        token_updates: Pin<Box<dyn Stream<Item = Vec<PairsWithPrice>> + 'static>>,
        pool_store: Arc<AngstromPoolConfigStore>
    ) -> Self {
        let (tx, rx) = unbounded_channel();
        let config_path = Path::new("./state_config.toml");
        let validation_config = load_validation_config(config_path).unwrap();

        tracing::debug!(?validation_config);
        let current_block =
            Arc::new(AtomicU64::new(BlockNumReader::best_block_number(&db).unwrap()));
        let db = Arc::new(db);

        let fetch = FetchUtils::new(Address::default(), db.clone());
        let pools = AngstromPoolsTracker::new(angstrom_address, pool_store);

        let handle = tokio::runtime::Handle::current();
        let thread_pool =
            KeySplitThreadpool::new(handle, validation_config.max_validation_per_user);
        let sim = SimValidation::new(db.clone(), None);

        // fill stream

        let order_validator = OrderValidator::new(
            sim,
            current_block,
            pools,
            fetch,
            uniswap_pools,
            thread_pool,
            token_conversion,
            token_updates
        )
        .await;

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
