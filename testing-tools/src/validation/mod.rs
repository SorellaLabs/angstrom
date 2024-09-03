use std::{
    future::{poll_fn, Future},
    path::Path,
    pin::Pin,
    sync::{atomic::AtomicU64, Arc},
    task::Poll,
    time::Duration
};

use alloy_primitives::{Address, U256};
use angstrom_eth::manager::EthEvent;
use futures::{FutureExt, Stream};
use reth_provider::StateProviderFactory;
use tokio::sync::mpsc::unbounded_channel;
use validation::{
    common::lru_db::RevmLRU,
    order::state::{
        config::{load_validation_config, ValidationConfig},
        db_state_utils::nonces::Nonces
    },
    validator::{ValidationClient, Validator}
};

use crate::mocks::eth_events::MockEthEventHandle;

type ValidatorOperation<DB, T> =
    dyn FnOnce(
        TestOrderValidator<DB>,
        T
    ) -> Pin<Box<dyn Future<Output = (TestOrderValidator<DB>, T)>>>;

pub struct TestOrderValidator<DB: StateProviderFactory + Clone + Unpin + 'static> {
    /// allows us to set values to ensure
    pub revm_lru:   Arc<RevmLRU<DB>>,
    pub config:     ValidationConfig,
    pub client:     ValidationClient,
    pub underlying: Validator<DB>,
    pub eth_handle: MockEthEventHandle
}

impl<DB: StateProviderFactory + Clone + Unpin + 'static> TestOrderValidator<DB> {
    pub fn new(
        db: DB,
        eth_stream: Pin<Box<dyn Stream<Item = EthEvent> + Send + Unpin>>,
        eth_handle: MockEthEventHandle
    ) -> Self {
        let (tx, rx) = unbounded_channel();
        let config_path = Path::new("./state_config.toml");
        let config = load_validation_config(config_path).unwrap();
        tracing::debug!(?config);
        let current_block = Arc::new(AtomicU64::new(db.best_block_number().unwrap()));
        let revm_lru = Arc::new(RevmLRU::new(10000000, Arc::new(db), current_block.clone()));

        let task_db = revm_lru.clone();

        let val = Validator::new(rx, eth_stream, task_db, config.clone(), current_block.clone());
        let client = ValidationClient(tx);

        Self { revm_lru, client, underlying: val, config, eth_handle }
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

pub struct OrderValidatorChain<DB: StateProviderFactory + Clone + Unpin + 'static, T: 'static> {
    validator:     TestOrderValidator<DB>,
    state:         T,
    operations:    Vec<Box<ValidatorOperation<DB, T>>>,
    poll_duration: Duration
}

impl<DB: StateProviderFactory + Clone + Unpin + 'static, T: 'static> OrderValidatorChain<DB, T> {
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
