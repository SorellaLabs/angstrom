use std::{
    collections::HashMap,
    sync::Arc
};

use alloy::primitives::{Address, B256, FixedBytes};
use angstrom_eth::manager::EthEvent;
use angstrom_types::{
    block_sync::BlockSyncConsumer,
    orders::{CancelOrderRequest, OrderLocation, OrderOrigin, OrderStatus},
    primitive::{OrderValidationError, PeerId},
    sol_bindings::grouped_orders::AllOrders
};
use futures::{Future, FutureExt};
use order_pool::{
    OrderIndexer, OrderPoolHandle, PoolConfig, PoolManagerUpdate,
    order_storage::OrderStorage
};
use reth_metrics::common::mpsc::UnboundedMeteredReceiver;
use reth_tasks::TaskSpawner;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, error::SendError};
use tokio_stream::wrappers::{BroadcastStream, UnboundedReceiverStream};
use validation::order::{OrderValidationResults, OrderValidatorHandle};

use crate::cache::LruCache;

use angstrom_types::network::{NetworkOrderEvent, StromNetworkEvent, NetworkHandle};

pub(crate) const MODULE_NAME: &str = "Order Pool";

/// Cache limit of transactions to keep track of for a single peer.
pub(crate) const PEER_ORDER_CACHE_LIMIT: usize = 1024 * 10;

/// Api to interact with [`PoolManager`] task.
#[derive(Debug, Clone)]
pub struct PoolHandle {
    pub manager_tx:      UnboundedSender<OrderCommand>,
    pub pool_manager_tx: tokio::sync::broadcast::Sender<PoolManagerUpdate>
}

#[derive(Debug)]
pub enum OrderCommand {
    // new orders
    NewOrder(OrderOrigin, AllOrders, tokio::sync::oneshot::Sender<OrderValidationResults>),
    CancelOrder(CancelOrderRequest, tokio::sync::oneshot::Sender<bool>),
    PendingOrders(Address, tokio::sync::oneshot::Sender<Vec<AllOrders>>),
    OrdersByPool(FixedBytes<32>, OrderLocation, tokio::sync::oneshot::Sender<Vec<AllOrders>>),
    OrderStatus(B256, tokio::sync::oneshot::Sender<Option<OrderStatus>>)
}

impl PoolHandle {
    fn send(&self, cmd: OrderCommand) -> Result<(), SendError<OrderCommand>> {
        self.manager_tx.send(cmd)
    }
}

impl OrderPoolHandle for PoolHandle {
    fn new_order(
        &self,
        origin: OrderOrigin,
        order: AllOrders
    ) -> impl Future<Output = Result<FixedBytes<32>, OrderValidationError>> + Send {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let order_hash = order.order_hash();
        let _ = self.send(OrderCommand::NewOrder(origin, order, tx));
        rx.map(move |res| {
            let Ok(result) = res else {
                return Err(OrderValidationError::Unknown {
                    err: "a channel failed on the backend".to_string()
                });
            };
            match result {
                OrderValidationResults::TransitionedToBlock(_)
                | OrderValidationResults::Valid(_) => Ok(order_hash),
                OrderValidationResults::Invalid { error, .. } => Err(error)
            }
        })
    }

    fn subscribe_orders(&self) -> BroadcastStream<PoolManagerUpdate> {
        BroadcastStream::new(self.pool_manager_tx.subscribe())
    }

    fn fetch_orders_from_pool(
        &self,
        pool_id: FixedBytes<32>,
        location: OrderLocation
    ) -> impl Future<Output = Vec<AllOrders>> + Send {
        let (tx, rx) = tokio::sync::oneshot::channel();

        let _ = self
            .manager_tx
            .send(OrderCommand::OrdersByPool(pool_id, location, tx));

        rx.map(|v| v.unwrap_or_default())
    }

    fn fetch_order_status(
        &self,
        order_hash: B256
    ) -> impl Future<Output = Option<OrderStatus>> + Send {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let _ = self
            .manager_tx
            .send(OrderCommand::OrderStatus(order_hash, tx));

        rx.map(|v| v.ok().flatten())
    }

    fn pending_orders(&self, sender: Address) -> impl Future<Output = Vec<AllOrders>> + Send {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let _ = self.send(OrderCommand::PendingOrders(sender, tx)).is_ok();
        rx.map(|res| res.unwrap_or_default())
    }

    fn cancel_order(&self, req: CancelOrderRequest) -> impl Future<Output = bool> + Send {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let _ = self.send(OrderCommand::CancelOrder(req, tx));
        rx.map(|res| res.unwrap_or(false))
    }
}

pub struct PoolManagerBuilder<V, GlobalSync, M = crate::consensus::ConsensusMode>
where
    V: OrderValidatorHandle,
    GlobalSync: BlockSyncConsumer
{
    validator:            V,
    global_sync:          GlobalSync,
    order_storage:        Option<Arc<OrderStorage>>,
    network_handle:       Box<dyn NetworkHandle + Send + Sync + 'static>,
    strom_network_events: UnboundedReceiverStream<StromNetworkEvent>,
    eth_network_events:   UnboundedReceiverStream<EthEvent>,
    order_events:         UnboundedMeteredReceiver<NetworkOrderEvent>,
    config:               PoolConfig,
    mode:                 M
}


impl<V, GlobalSync, M> PoolManagerBuilder<V, GlobalSync, M>
where
    V: OrderValidatorHandle<Order = AllOrders> + Unpin,
    GlobalSync: BlockSyncConsumer
{
    pub fn new<NH: NetworkHandle + Send + Sync + 'static>(
        validator: V,
        order_storage: Option<Arc<OrderStorage>>,
        network_handle: NH,
        eth_network_events: UnboundedReceiverStream<EthEvent>,
        order_events: UnboundedMeteredReceiver<NetworkOrderEvent>,
        global_sync: GlobalSync,
        mode: M
    ) -> Self {
        Self {
            order_events,
            global_sync,
            eth_network_events,
            strom_network_events: network_handle.subscribe_network_events(),
            network_handle: Box::new(network_handle),
            validator,
            order_storage,
            config: Default::default(),
            mode
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
    ) -> PoolHandle
    where
        M: Send + 'static,
        PoolManager<V, GlobalSync, M>: Future<Output = ()> + Send,
    {
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

        task_spawner.spawn_critical(
            "transaction manager",
            Box::pin(PoolManager {
                eth_network_events:   self.eth_network_events,
                strom_network_events: self.strom_network_events,
                order_events:         self.order_events,
                peer_to_info:         HashMap::default(),
                order_indexer:        inner,
                network:              self.network_handle,
                command_rx:           rx,
                global_sync:          self.global_sync,
                mode:                 self.mode
            })
        );

        handle
    }
}

pub struct PoolManager<V, GlobalSync, M = crate::consensus::ConsensusMode>
where
    V: OrderValidatorHandle,
    GlobalSync: BlockSyncConsumer
{
    /// access to validation and sorted storage of orders.
    pub(crate) order_indexer:        OrderIndexer<V>,
    pub(crate) global_sync:          GlobalSync,
    /// Network access.
    pub(crate) network:              Box<dyn NetworkHandle + Send + Sync + 'static>,
    /// Subscriptions to all the strom-network related events.
    ///
    /// From which we get all new incoming order related messages.
    pub(crate) strom_network_events: UnboundedReceiverStream<StromNetworkEvent>,
    /// Ethereum updates stream that tells the pool manager about orders that
    /// have been filled  
    pub(crate) eth_network_events:   UnboundedReceiverStream<EthEvent>,
    /// receiver half of the commands to the pool manager
    pub(crate) command_rx:           UnboundedReceiverStream<OrderCommand>,
    /// Incoming events from the ProtocolManager.
    pub(crate) order_events:         UnboundedMeteredReceiver<NetworkOrderEvent>,
    /// All the connected peers.
    pub(crate) peer_to_info:         HashMap<PeerId, StromPeer>,
    /// Mode-specific state and behavior
    pub(crate) mode:                 M
}

// Generic impl block removed - methods are now in mode-specific implementations

// Note: Future implementation is now moved to mode-specific impl blocks
// in consensus.rs and rollup.rs modules

/// All events related to orders emitted by the network.
#[derive(Debug)]
#[allow(missing_docs)]
pub enum NetworkTransactionEvent {
    /// Received list of transactions from the given peer.
    ///
    /// This represents transactions that were broadcasted to use from the peer.
    IncomingOrders { peer_id: PeerId, msg: Vec<AllOrders> }
}

/// Tracks a single peer
#[derive(Debug)]
pub(crate) struct StromPeer {
    /// Keeps track of transactions that we know the peer has seen.
    pub(crate) orders:        LruCache<B256>,
    pub(crate) cancellations: LruCache<B256>
}

// Type aliases for convenience
/// Pool manager configured for consensus mode - includes consensus-specific behavior
pub type ConsensusPoolManager<V, GlobalSync> = PoolManager<V, GlobalSync, crate::consensus::ConsensusMode>;

/// Pool manager configured for rollup mode - simpler behavior without consensus logic
pub type RollupPoolManager<V, GlobalSync> = PoolManager<V, GlobalSync, crate::rollup::RollupMode>;

/// Pool manager builder configured for consensus mode
pub type ConsensusPoolManagerBuilder<V, GlobalSync> = PoolManagerBuilder<V, GlobalSync, crate::consensus::ConsensusMode>;

/// Pool manager builder configured for rollup mode
pub type RollupPoolManagerBuilder<V, GlobalSync> = PoolManagerBuilder<V, GlobalSync, crate::rollup::RollupMode>;

// Mode-specific constructor implementations
impl<V, GlobalSync> ConsensusPoolManagerBuilder<V, GlobalSync>
where
    V: OrderValidatorHandle<Order = AllOrders> + Unpin,
    GlobalSync: BlockSyncConsumer
{
    /// Create a new consensus pool manager builder
    pub fn new_consensus<NH: NetworkHandle + Send + Sync + 'static>(
        validator: V,
        order_storage: Option<Arc<OrderStorage>>,
        network_handle: NH,
        eth_network_events: UnboundedReceiverStream<EthEvent>,
        order_events: UnboundedMeteredReceiver<NetworkOrderEvent>,
        global_sync: GlobalSync
    ) -> Self {
        Self::new(
            validator,
            order_storage,
            network_handle,
            eth_network_events,
            order_events,
            global_sync,
            crate::consensus::ConsensusMode::new()
        )
    }
}

impl<V, GlobalSync> RollupPoolManagerBuilder<V, GlobalSync>
where
    V: OrderValidatorHandle<Order = AllOrders> + Unpin,
    GlobalSync: BlockSyncConsumer
{
    /// Create a new rollup pool manager builder
    pub fn new_rollup<NH: NetworkHandle + Send + Sync + 'static>(
        validator: V,
        order_storage: Option<Arc<OrderStorage>>,
        network_handle: NH,
        eth_network_events: UnboundedReceiverStream<EthEvent>,
        order_events: UnboundedMeteredReceiver<NetworkOrderEvent>,
        global_sync: GlobalSync
    ) -> Self {
        Self::new(
            validator,
            order_storage,
            network_handle,
            eth_network_events,
            order_events,
            global_sync,
            crate::rollup::RollupMode::new()
        )
    }
}