use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll}
};

use angstrom_eth::manager::EthEvent;
use angstrom_network::{NetworkHandle, StromNetworkEvent};
use angstrom_types::{
    block_sync::BlockSyncConsumer,
    network::{PoolNetworkMessage, ReputationChangeKind},
    primitive::PeerId,
    sol_bindings::grouped_orders::AllOrders
};
use futures::{Future, StreamExt};
use order_pool::{
    OrderIndexer, PoolConfig, PoolInnerEvent, PoolManagerUpdate, order_storage::OrderStorage
};
use reth_tasks::TaskSpawner;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio_stream::wrappers::UnboundedReceiverStream;
use validation::order::OrderValidatorHandle;

use crate::{
    PoolManagerMode,
    order::{MODULE_NAME, OrderCommand, PoolHandle, PoolManager}
};

/// Unit type used as a placeholder for network handle in rollup mode
#[derive(Debug, Clone, Copy)]
pub struct NoNetwork;

impl NetworkHandle for NoNetwork {
    type Events<'a>
        = UnboundedReceiverStream<StromNetworkEvent>
    where
        Self: 'a;

    fn send_message(&mut self, _peer_id: PeerId, _message: PoolNetworkMessage) {
        // No-op for rollup mode
    }

    fn peer_reputation_change(&mut self, _peer_id: PeerId, _change: ReputationChangeKind) {
        // No-op for rollup mode
    }

    fn subscribe_network_events(&self) -> Self::Events<'_> {
        // Return an empty receiver since rollup mode doesn't have network events
        let (_, rx) = tokio::sync::mpsc::unbounded_channel();
        UnboundedReceiverStream::new(rx)
    }
}

/// Type alias for rollup pool manager
pub type RollupPoolManager<V, GS> = PoolManager<V, GS, NoNetwork, RollupMode>;

/// Builder for constructing RollupPoolManager instances.
pub struct RollupPoolManagerBuilder<V, GlobalSync>
where
    V: OrderValidatorHandle,
    GlobalSync: BlockSyncConsumer
{
    validator:          V,
    global_sync:        GlobalSync,
    order_storage:      Option<Arc<OrderStorage>>,
    eth_network_events: UnboundedReceiverStream<EthEvent>,
    config:             PoolConfig
}

impl<V, GlobalSync> RollupPoolManagerBuilder<V, GlobalSync>
where
    V: OrderValidatorHandle<Order = AllOrders> + Unpin,
    GlobalSync: BlockSyncConsumer
{
    pub fn new(
        validator: V,
        order_storage: Option<Arc<OrderStorage>>,
        eth_network_events: UnboundedReceiverStream<EthEvent>,
        global_sync: GlobalSync
    ) -> Self {
        Self {
            global_sync,
            eth_network_events,
            validator,
            order_storage,
            config: Default::default()
        }
    }

    pub fn with_config(mut self, config: PoolConfig) -> Self {
        self.config = config;
        self
    }

    pub fn with_storage(mut self, order_storage: Arc<OrderStorage>) -> Self {
        let _ = self.order_storage.insert(order_storage);
        self
    }

    pub fn build_with_channels<TP: TaskSpawner>(
        self,
        task_spawner: TP,
        tx: UnboundedSender<OrderCommand>,
        rx: UnboundedReceiver<OrderCommand>,
        pool_manager_tx: tokio::sync::broadcast::Sender<PoolManagerUpdate>,
        block_number: u64,
        replay: impl FnOnce(&mut OrderIndexer<V>) + Send + 'static
    ) -> PoolHandle {
        let rx = UnboundedReceiverStream::new(rx);
        let order_storage = self
            .order_storage
            .unwrap_or_else(|| Arc::new(OrderStorage::new(&self.config)));
        let handle =
            PoolHandle { manager_tx: tx.clone(), pool_manager_tx: pool_manager_tx.clone() };
        let mut inner = OrderIndexer::new(
            self.validator.clone(),
            order_storage.clone(),
            block_number,
            pool_manager_tx.clone()
        );
        replay(&mut inner);
        self.global_sync.register(MODULE_NAME);

        let mode = RollupMode::new();

        task_spawner.spawn_critical(
            "order pool manager",
            Box::pin(PoolManager::<V, GlobalSync, NoNetwork, RollupMode> {
                eth_network_events: self.eth_network_events,
                order_indexer: inner,
                network: NoNetwork,
                command_rx: rx,
                global_sync: self.global_sync,
                mode
            })
        );

        handle
    }
}

impl<V, GS> RollupPoolManager<V, GS>
where
    V: OrderValidatorHandle<Order = AllOrders> + Unpin,
    GS: BlockSyncConsumer
{
    /// Create a new rollup pool manager builder
    pub fn new(
        validator: V,
        order_storage: Option<Arc<OrderStorage>>,
        eth_network_events: UnboundedReceiverStream<EthEvent>,
        global_sync: GS
    ) -> RollupPoolManagerBuilder<V, GS> {
        RollupPoolManagerBuilder::new(validator, order_storage, eth_network_events, global_sync)
    }
}

/// Rollup mode for PoolManager - simpler behavior without consensus logic
#[derive(Debug)]
pub struct RollupMode;

// RollupMode automatically derives Send + Sync since it has no fields

impl RollupMode {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RollupMode {
    fn default() -> Self {
        Self::new()
    }
}

impl PoolManagerMode for RollupMode {
    fn get_proposable_orders<V, GS, NH>(pool: &mut PoolManager<V, GS, NH, Self>) -> Vec<AllOrders>
    where
        V: OrderValidatorHandle<Order = AllOrders> + Unpin,
        GS: BlockSyncConsumer,
        NH: NetworkHandle,
        Self: Sized
    {
        // In rollup mode, we process all orders without consensus filtering
        // This is typically simpler and more direct
        pool.order_indexer
            .get_all_orders_with_parked()
            .into_all_orders()
    }

    // The default no-op for poll_mode_specific is sufficient for RollupMode
    // as it doesn't need any mode-specific polling behavior.
    // Specifically, RollupMode does NOT poll network events since it operates
    // without direct peer networking
}

impl<V, GlobalSync> PoolManager<V, GlobalSync, NoNetwork, RollupMode>
where
    V: OrderValidatorHandle<Order = AllOrders> + Unpin,
    GlobalSync: BlockSyncConsumer
{
    /// Rollup-specific order processing logic
    ///
    /// This method provides a convenient way to get proposable orders
    /// for rollup mode, which processes all orders without consensus filtering.
    pub fn get_rollup_orders(&mut self) -> Vec<AllOrders> {
        RollupMode::get_proposable_orders(self)
    }

    fn on_pool_events(
        &mut self,
        orders: Vec<PoolInnerEvent>,
        waker: impl Fn() -> std::task::Waker
    ) {
        for order in orders {
            match order {
                PoolInnerEvent::Propagation(_order) => {
                    // In rollup mode, no need to broadcast orders to peers
                    // since there's no networking
                }
                PoolInnerEvent::BadOrderMessages(_o) => {
                    // In rollup mode, we don't have networking, so no
                    // reputation changes
                }
                PoolInnerEvent::HasTransitionedToNewBlock(block) => {
                    self.global_sync.sign_off_on_block(
                        crate::order::MODULE_NAME,
                        block,
                        Some(waker())
                    );
                }
                PoolInnerEvent::None => {}
            }
        }
    }

    fn on_eth_event(&mut self, eth: EthEvent, waker: std::task::Waker) {
        use angstrom_types::primitive::{NewInitializedPool, PoolId};

        match eth {
            EthEvent::NewBlockTransitions { block_number, filled_orders, address_changeset } => {
                self.order_indexer.start_new_block_processing(
                    block_number,
                    filled_orders,
                    address_changeset
                );
                waker.clone().wake_by_ref();
            }
            EthEvent::ReorgedOrders(orders, range) => {
                self.order_indexer.reorg(orders);
                self.global_sync
                    .sign_off_reorg(crate::order::MODULE_NAME, range, Some(waker))
            }
            EthEvent::FinalizedBlock(block) => {
                self.order_indexer.finalized_block(block);
            }
            EthEvent::NewPool { pool } => {
                let t0 = pool.currency0;
                let t1 = pool.currency1;
                let id: PoolId = pool.into();

                let pool = NewInitializedPool { currency_in: t0, currency_out: t1, id };

                self.order_indexer.new_pool(pool);
            }
            EthEvent::RemovedPool { pool } => {
                self.order_indexer.remove_pool(pool.into());
            }
            EthEvent::AddedNode(_) => {}
            EthEvent::RemovedNode(_) => {}
            EthEvent::NewBlock(_) => {}
        }
    }

    fn on_command(&mut self, cmd: OrderCommand) {
        use angstrom_types::orders::OrderOrigin;
        use telemetry_recorder::telemetry_event;

        match cmd {
            OrderCommand::NewOrder(origin, order, validation_response) => {
                let blocknum = self.global_sync.current_block_number();
                telemetry_event!(blocknum, origin, order.clone());

                self.order_indexer
                    .new_rpc_order(OrderOrigin::External, order, validation_response)
            }
            OrderCommand::CancelOrder(req, receiver) => {
                let blocknum = self.global_sync.current_block_number();
                telemetry_event!(blocknum, req.clone());

                let res = self.order_indexer.cancel_order(&req);
                // In rollup mode, no need to broadcast cancellation to peers
                let _ = receiver.send(res);
            }
            OrderCommand::PendingOrders(from, receiver) => {
                let res = self.order_indexer.pending_orders_for_address(from);
                let _ = receiver.send(res.into_iter().map(|o| o.order).collect());
            }
            OrderCommand::OrderStatus(order_hash, tx) => {
                let res = self.order_indexer.order_status(order_hash);
                let _ = tx.send(res);
            }
            OrderCommand::OrdersByPool(pool_id, location, tx) => {
                let res = self.order_indexer.orders_by_pool(pool_id, location);
                let _ = tx.send(res);
            }
        }
    }
}

impl<V, GlobalSync> Future for PoolManager<V, GlobalSync, NoNetwork, RollupMode>
where
    V: OrderValidatorHandle<Order = AllOrders> + Unpin,
    GlobalSync: BlockSyncConsumer
{
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        let mut work = 30;
        loop {
            work -= 1;
            if work == 0 {
                cx.waker().wake_by_ref();
                break;
            }

            // pull all eth events
            while let Poll::Ready(Some(eth)) = this.eth_network_events.poll_next_unpin(cx) {
                this.on_eth_event(eth, cx.waker().clone());
            }

            // poll underlying pool. This is the validation process that's being polled
            while let Poll::Ready(Some(orders)) = this.order_indexer.poll_next_unpin(cx) {
                this.on_pool_events(orders, || cx.waker().clone());
            }

            // Poll mode-specific events (no-op for RollupMode)
            RollupMode::poll_mode_specific(this, cx);

            // halt dealing with these till we have synced
            if this.global_sync.can_operate() {
                // drain commands
                if let Poll::Ready(Some(cmd)) = this.command_rx.poll_next_unpin(cx) {
                    this.on_command(cmd);
                    cx.waker().wake_by_ref();
                }
            }
        }

        Poll::Pending
    }
}
