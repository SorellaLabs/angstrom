use std::{pin::Pin, task::Poll, time::Duration};

use angstrom_eth::manager::EthEvent;
use angstrom_network::{
    pool_manager::{PoolHandle, PoolManager},
    NetworkOrderEvent, StromNetworkEvent, StromNetworkHandle
};
use angstrom_types::rpc::{
    EcRecoveredComposableLimitOrder, EcRecoveredComposableSearcherOrder, EcRecoveredLimitOrder,
    EcRecoveredSearcherOrder
};
use futures::{future::poll_fn, Future, FutureExt};
use order_pool::{OrderPoolInner, PoolConfig};
use reth_metrics::common::mpsc::UnboundedMeteredReceiver;
use tokio::sync::mpsc::unbounded_channel;
use tokio_stream::wrappers::UnboundedReceiverStream;

use crate::mocks::validator::MockValidator;

type DefaultMockPoolManager = PoolManager<
    EcRecoveredLimitOrder,
    EcRecoveredComposableLimitOrder,
    EcRecoveredSearcherOrder,
    EcRecoveredComposableSearcherOrder,
    MockValidator
>;

type DefaultPoolHandle = PoolHandle<
    EcRecoveredLimitOrder,
    EcRecoveredComposableLimitOrder,
    EcRecoveredSearcherOrder,
    EcRecoveredComposableSearcherOrder
>;
/// The Testnet orderpool allows us to test the orderpool functionality in a
/// standalone and an iterop mode. what this means is we can use this for
/// specific unit tests aswell as longer full range tests
pub struct TestnetOrderPool {
    pub pool_manager: DefaultMockPoolManager,
    pub pool_handle:  DefaultPoolHandle
}

impl TestnetOrderPool {
    pub fn new_full_mock(
        validator: MockValidator,
        config: PoolConfig,
        network_handle: StromNetworkHandle,
        eth_network_events: UnboundedReceiverStream<EthEvent>,
        order_events: UnboundedMeteredReceiver<NetworkOrderEvent>,
        strom_network_events: UnboundedReceiverStream<StromNetworkEvent>
    ) -> Self {
        let (tx, rx) = unbounded_channel();
        let rx = UnboundedReceiverStream::new(rx);
        let handle = PoolHandle { manager_tx: tx.clone() };
        let inner = OrderPoolInner::new(validator, config);

        Self {
            pool_manager: PoolManager::new(
                inner,
                network_handle,
                strom_network_events,
                eth_network_events,
                tx,
                rx,
                order_events
            ),
            pool_handle:  handle
        }
    }

    pub fn get_handle(&self) -> &DefaultPoolHandle {
        &self.pool_handle
    }

    pub async fn poll_until<F: FnMut() -> bool>(&mut self, mut finished: F) -> bool {
        poll_fn(|cx| {
            if self.pool_manager.poll_unpin(cx).is_ready() {
                return Poll::Ready(false)
            }

            if finished() {
                return Poll::Ready(true)
            } else {
                cx.waker().wake_by_ref();
            }
            Poll::Pending
        })
        .await
    }

    pub async fn poll_for(&mut self, duration: Duration) {
        let _ = tokio::time::timeout(
            duration,
            poll_fn(|cx| {
                if self.pool_manager.poll_unpin(cx).is_ready() {
                    return Poll::Ready(())
                }
                cx.waker().wake_by_ref();
                Poll::Pending
            })
        )
        .await;
    }
}

/// allows for channing operations with a poll time between operations. Is
/// helpful for it testing async functions.
pub struct OperationChainer<T: 'static> {
    pool:          TestnetOrderPool,
    state:         T,
    operations: Vec<
        Box<
            dyn FnOnce(TestnetOrderPool, T) -> Pin<Box<dyn Future<Output = (TestnetOrderPool, T)>>>
        >
    >,
    poll_duration: Duration
}

impl<T: 'static> OperationChainer<T> {
    pub fn new(pool: TestnetOrderPool, poll_duration: Duration, state: T) -> Self {
        Self { poll_duration, pool, operations: vec![], state }
    }

    pub fn add_operation<F>(mut self, op: F) -> Self
    where
        F: FnOnce(TestnetOrderPool, T) -> Pin<Box<dyn Future<Output = (TestnetOrderPool, T)>>>
            + 'static
    {
        self.operations.push(Box::new(op));
        self
    }

    pub async fn execute_all_operations(self) {
        let (mut pool, mut state) = (self.pool, self.state);

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
