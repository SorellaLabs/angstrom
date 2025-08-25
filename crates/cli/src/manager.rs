use std::{
    collections::HashMap,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Waker},
    time::Duration
};

use alloy::{consensus::BlockHeader, providers::Provider};
use alloy_primitives::{Address, BlockNumber};
use angstrom_types::{
    block_sync::BlockSyncConsumer,
    contract_payloads::angstrom::{AngstromBundle, BundleGasDetails, UniswapAngstromRegistry},
    orders::{PoolSolution, unique_searcher_orders_per_pool},
    primitive::{AngstromMetaSigner, AngstromSigner, ChainExt, PoolId},
    submission::SubmissionHandler,
    uni_structure::{BaselinePoolState, PoolSnapshots}
};
use futures::{FutureExt, StreamExt, future::BoxFuture};
use matching_engine::MatchingEngineHandle;
use order_pool::order_storage::OrderStorage;
use reth::tasks::shutdown::GracefulShutdown;
use reth_optimism_primitives::OpPrimitives;
use reth_provider::CanonStateNotification;
use tokio::time::Sleep;
use tokio_stream::wrappers::BroadcastStream;
use uniswap_v4::uniswap::pool_manager::SyncedUniswapPools;

// TODO(mempirate): Is this a good value?
/// The factor by which the block time is multiplied to get the bid aggregation
/// deadline. = 80% of the block time.
///
/// NOTE: On lower block times (like Flashblocks, we might have to decrease this
/// to account for transaction inclusion latency (network + processing)).
const BID_AGGREGATION_DEADLINE_FACTOR: f64 = 0.8;

/// The runtime driver for rollup mode. Advances the global Angstrom state
/// machine without consensus or networking.
pub struct RollupManager<P, M, BS, S>
where
    P: Provider + Unpin + 'static,
    S: AngstromMetaSigner
{
    current_height:         BlockNumber,
    block_time:             Duration,
    // TODO(mempirate): If we make this a generic driver, don't use concrete type here
    canonical_block_stream: BroadcastStream<CanonStateNotification<OpPrimitives>>,
    block_sync:             BS,
    /// Contains all orders that came in through the RPC.
    order_storage:          Arc<OrderStorage>,
    pool_registry:          UniswapAngstromRegistry,
    uniswap_pools:          SyncedUniswapPools,
    provider:               Arc<SubmissionHandler<P>>,
    matching_engine:        M,
    signer:                 AngstromSigner<S>,

    state: DriverState
}

/// The driver state machine.
enum DriverState {
    /// The driver is waiting for a new block to start the
    /// [`DriverState::BidAggregation`] state.
    Waiting,
    /// The driver is waiting for the bid aggregation deadline to pass. After
    /// this it will move on to the [`DriverState::Solving`] state.
    BidAggregation {
        /// The sleep future to wait for the bid aggregation deadline.
        /// NOTE: This is boxed and pinned to make it `Unpin`.
        sleep: Pin<Box<Sleep>>
    },
    /// The driver is solving the book. When the solution is found, it will
    /// reset to the [`DriverState::Waiting`] state.
    Solving {
        future:
            BoxFuture<'static, eyre::Result<(PoolSnapshots, Vec<PoolSolution>, BundleGasDetails)>>
    }
}

impl DriverState {
    /// Poll the sleep future if in the [`DriverState::BidAggregation`] state.
    /// Returns `Poll::Ready` when the sleep is complete,
    /// otherwise `Poll::Pending`. Also returns `Poll::Pending` if the driver is
    /// not in the `BidAggregation` state.
    fn poll_sleep(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        match self {
            DriverState::BidAggregation { sleep } => sleep.poll_unpin(cx),
            _ => Poll::Pending
        }
    }

    /// Poll the solving future if in the [`DriverState::Solving`] state.
    fn poll_solver(
        &mut self,
        cx: &mut Context<'_>
    ) -> Poll<eyre::Result<(PoolSnapshots, Vec<PoolSolution>, BundleGasDetails)>> {
        match self {
            DriverState::Solving { future } => future.poll_unpin(cx),
            _ => Poll::Pending
        }
    }

    /// Reset the driver state to [`DriverState::BidAggregation`] with a new
    /// timeout.
    fn reset(&mut self, timeout: Duration) {
        match &self {
            DriverState::BidAggregation { .. } => {
                tracing::error!("Invalid state transition: `BidAggregation` -> `BidAggregation`");
            }
            DriverState::Solving { .. } => {
                tracing::error!("Invalid state transition: `Solving` -> `BidAggregation`");
            }
            DriverState::Waiting => {
                tracing::debug!("Reset driver state: `Waiting` -> `BidAggregation`");
            }
        }

        *self = DriverState::BidAggregation { sleep: Box::pin(tokio::time::sleep(timeout)) };
    }
}

impl<P, M, BS, S> RollupManager<P, M, BS, S>
where
    P: Provider + Unpin + 'static,
    M: MatchingEngineHandle,
    BS: BlockSyncConsumer,
    S: AngstromMetaSigner
{
    /// Initialize a new [`RollupManager`] instance.
    pub fn new(
        current_height: BlockNumber,
        block_time: Duration,
        canonical_block_stream: BroadcastStream<CanonStateNotification<OpPrimitives>>,
        block_sync: BS,
        order_storage: Arc<OrderStorage>,
        pool_registry: UniswapAngstromRegistry,
        uniswap_pools: SyncedUniswapPools,
        provider: Arc<SubmissionHandler<P>>,
        matching_engine: M,
        signer: AngstromSigner<S>
    ) -> Self {
        block_sync.register("RollupManager");

        Self {
            current_height,
            block_time,
            canonical_block_stream,
            block_sync,
            order_storage,
            pool_registry,
            uniswap_pools,
            provider,
            matching_engine,
            signer,
            state: DriverState::Waiting
        }
    }

    /// Handles a new blockchain state notification.
    fn on_blockchain_state(
        &mut self,
        notification: CanonStateNotification<OpPrimitives>,
        waker: Waker
    ) {
        let new_block = notification.tip();
        let number = new_block.number();

        tracing::info!(?number, "New blockchain state");
        self.current_height = number;

        // Reset the timeout.
        self.state
            .reset(self.block_time.mul_f64(BID_AGGREGATION_DEADLINE_FACTOR));

        match notification {
            CanonStateNotification::Commit { .. } => {
                self.block_sync.sign_off_on_block(
                    "RollupManager",
                    self.current_height,
                    Some(waker)
                );
            }

            CanonStateNotification::Reorg { old, new } => {
                let tip = new.tip_number();
                let reorg = old.reorged_range(&new).unwrap_or(tip..=tip);
                self.block_sync
                    .sign_off_reorg("RollupManager", reorg, Some(waker));
            }
        }
    }

    /// Handles the bid aggregation deadline: starts the solving process.
    fn on_aggregation_deadline(&mut self) {
        let orders = self.order_storage.get_all_orders();

        let (limit, searcher) = orders.into_all_book_and_searcher();

        let searcher = unique_searcher_orders_per_pool(searcher);

        let pool_snapshots = self.fetch_pool_snapshot();
        let matcher = self.matching_engine.clone();

        let future = async move {
            let (solutions, details) = matcher
                .solve_pools(limit, searcher, pool_snapshots.clone())
                .await?;

            Ok((pool_snapshots, solutions, details))
        }
        .boxed();

        // Change state to solving.
        self.state = DriverState::Solving { future };
    }

    fn on_solving_result(
        &mut self,
        pool_snapshots: PoolSnapshots,
        mut pool_solutions: Vec<PoolSolution>,
        details: BundleGasDetails
    ) {
        // Sort the pool solutions by pool id (expected in some of the calls below).
        pool_solutions.sort();

        let all_orders = self
            .order_storage
            .get_all_orders_with_ingoing_cancellations();

        // Build the Angstrom bundle from the pool solutions.
        tracing::trace!(height = self.current_height, "Building bundle");
        let bundle = match AngstromBundle::from_pool_solutions(
            pool_solutions,
            all_orders,
            &pool_snapshots,
            details
        ) {
            Ok(bundle) => bundle,
            Err(e) => {
                tracing::error!(
                    ?e,
                    height = self.current_height,
                    "Error building bundle! No bundle will be submitted"
                );
                return;
            }
        };

        // Reset the state to waiting. State transition cycle is now complete, ready for
        // the next block to start the next cycle.
        self.state = DriverState::Waiting;

        let target_block = self.current_height + 1;
        let provider = self.provider.clone();
        let signer = self.signer.clone();
        let height = self.current_height;

        // NOTE: We can just spawn the submission here. We don't need to store the
        // result, since in rollup mode we don't do anything with it.
        tokio::spawn(async move {
            match provider.submit_tx(signer, Some(bundle), target_block).await {
                Ok(Some(tx_hash)) => {
                    tracing::info!(
                        height,
                        target_block,
                        ?tx_hash,
                        "Submitted bundle, waiting for inclusion..."
                    );

                    // Wait for the next block
                    provider
                        .watch_blocks()
                        .await
                        .unwrap()
                        .with_poll_interval(Duration::from_millis(10))
                        .into_stream()
                        .next()
                        .await;

                    if provider
                        .get_transaction_by_hash(tx_hash)
                        .await
                        .unwrap()
                        .is_some()
                    {
                        tracing::info!(target_block, ?tx_hash, "Bundle included in block");
                    } else {
                        tracing::error!(target_block, ?tx_hash, "Bundle not included in block");
                    }
                }
                Ok(None) => {
                    tracing::warn!(height, "Submitted bundle, but no tx hash returned");
                }
                Err(e) => {
                    tracing::error!(?e, height, "Error submitting bundle");
                }
            }
        });
    }

    fn fetch_pool_snapshot(&self) -> HashMap<PoolId, (Address, Address, BaselinePoolState, u16)> {
        self.uniswap_pools
            .iter()
            .filter_map(|item| {
                let key = item.key();
                let pool = item.value();
                tracing::info!(?key, "getting snapshot");
                let (token_a, token_b, snapshot) =
                    pool.read().unwrap().fetch_pool_snapshot().ok()?;
                let entry = self.pool_registry.get_ang_entry(key)?;

                Some((*key, (token_a, token_b, snapshot, entry.store_index as u16)))
            })
            .collect::<HashMap<_, _>>()
    }

    pub async fn run_till_shutdown(mut self, sig: GracefulShutdown) {
        tokio::select! {
            _ = &mut self => {
            }
            _ = sig => {
                tracing::info!("Shutting down RollupManager");
            }
        }
    }
}

impl<P, M, BS, S> Future for RollupManager<P, M, BS, S>
where
    P: Provider + Unpin + 'static,
    M: MatchingEngineHandle,
    BS: BlockSyncConsumer,
    S: AngstromMetaSigner
{
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        loop {
            // If this is ready, the bid aggregation deadline has passed and we proceed to
            // the next stage.
            if let Poll::Ready(()) = this.state.poll_sleep(cx) {
                this.on_aggregation_deadline();
            }

            if let Poll::Ready(result) = this.state.poll_solver(cx) {
                match result {
                    Ok((pool_snapshots, pool_solutions, details)) => {
                        this.on_solving_result(pool_snapshots, pool_solutions, details);
                    }
                    Err(e) => {
                        tracing::error!(
                            ?e,
                            height = this.current_height,
                            "Error solving pools, no bundle will be submitted!"
                        );
                    }
                }
            }

            match this.canonical_block_stream.poll_next_unpin(cx) {
                Poll::Ready(Some(Ok(notification))) => {
                    this.on_blockchain_state(notification, cx.waker().clone());

                    continue;
                }
                Poll::Ready(Some(Err(e))) => {
                    tracing::error!(?e, "Stream lagging behind");
                }
                Poll::Ready(None) => {
                    // We exit the rollup driver when we receive no more chain notifications.
                    tracing::warn!("No more chain state notifications");
                    return Poll::Ready(());
                }
                Poll::Pending => {}
            }

            return Poll::Pending;
        }
    }
}
