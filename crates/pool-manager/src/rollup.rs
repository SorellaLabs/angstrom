use std::{marker::PhantomData, sync::Arc};

use angstrom_eth::manager::EthEvent;
use angstrom_network::{NetworkHandle, NetworkOrderEvent, StromNetworkEvent};
use angstrom_types::{
    block_sync::BlockSyncConsumer,
    network::{PoolNetworkMessage, ReputationChangeKind},
    primitive::PeerId,
    sol_bindings::grouped_orders::AllOrders
};
use order_pool::{PoolConfig, order_storage::OrderStorage};
use reth_metrics::common::mpsc::UnboundedMeteredReceiver;
use reth_tasks::TaskSpawner;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio_stream::wrappers::UnboundedReceiverStream;
use validation::order::OrderValidatorHandle;

use order_pool::{OrderIndexer, PoolManagerUpdate};

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

/// A type alias for the rollup pool manager.
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
    order_events:       UnboundedMeteredReceiver<NetworkOrderEvent>,
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
        order_events: UnboundedMeteredReceiver<NetworkOrderEvent>,
        global_sync: GlobalSync
    ) -> Self {
        Self {
            order_events,
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

        // Create empty network event stream for rollup mode
        let (_, network_events_rx) = tokio::sync::mpsc::unbounded_channel();
        let strom_network_events = UnboundedReceiverStream::new(network_events_rx);

        task_spawner.spawn_critical(
            "order pool manager",
            Box::pin(PoolManager::<V, GlobalSync, NoNetwork, RollupMode> {
                eth_network_events: self.eth_network_events,
                strom_network_events,
                order_events: self.order_events,
                peer_to_info: std::collections::HashMap::default(),
                order_indexer: inner,
                network: NoNetwork,
                command_rx: rx,
                global_sync: self.global_sync,
                _mode: PhantomData
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
        order_events: UnboundedMeteredReceiver<NetworkOrderEvent>,
        global_sync: GS
    ) -> RollupPoolManagerBuilder<V, GS> {
        RollupPoolManagerBuilder::new(
            validator,
            order_storage,
            eth_network_events,
            order_events,
            global_sync
        )
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
    const REQUIRES_NETWORKING: bool = false;

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
}
