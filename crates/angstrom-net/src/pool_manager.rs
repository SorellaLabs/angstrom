use std::{
    collections::HashMap,
    num::NonZeroUsize,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Waker}
};

use alloy::primitives::{Address, FixedBytes, B256};
use angstrom_eth::manager::EthEvent;
use angstrom_types::{
    block_sync::BlockSyncConsumer,
    orders::{CancelOrderRequest, OrderLocation, OrderOrigin, OrderStatus},
    primitive::{NewInitializedPool, PeerId, PoolId},
    sol_bindings::grouped_orders::AllOrders
};
use futures::{Future, FutureExt, StreamExt};
use order_pool::{
    order_storage::OrderStorage, OrderIndexer, OrderPoolHandle, PoolConfig, PoolInnerEvent,
    PoolManagerUpdate
};
use reth_metrics::common::mpsc::UnboundedMeteredReceiver;
use reth_tasks::TaskSpawner;
use tokio::sync::{
    broadcast,
    mpsc::{error::SendError, unbounded_channel, UnboundedReceiver, UnboundedSender}
};
use tokio_stream::wrappers::{BroadcastStream, UnboundedReceiverStream};
use validation::order::{
    state::pools::AngstromPoolsTracker, OrderPoolNewOrderResult, OrderValidationResults,
    OrderValidatorHandle
};

use crate::{LruCache, NetworkOrderEvent, StromMessage, StromNetworkEvent, StromNetworkHandle};

const MODULE_NAME: &str = "Order Pool";

/// Cache limit of transactions to keep track of for a single peer.
const PEER_ORDER_CACHE_LIMIT: usize = 1024 * 10;

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
    ) -> impl Future<Output = OrderPoolNewOrderResult> + Send {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let _ = self.send(OrderCommand::NewOrder(origin, order, tx));
        rx.map(Into::into)
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

pub struct PoolManagerBuilder<V, GlobalSync>
where
    V: OrderValidatorHandle,
    GlobalSync: BlockSyncConsumer
{
    validator:            V,
    global_sync:          GlobalSync,
    order_storage:        Option<Arc<OrderStorage>>,
    network_handle:       StromNetworkHandle,
    strom_network_events: UnboundedReceiverStream<StromNetworkEvent>,
    eth_network_events:   UnboundedReceiverStream<EthEvent>,
    order_events:         UnboundedMeteredReceiver<NetworkOrderEvent>,
    config:               PoolConfig
}

impl<V, GlobalSync> PoolManagerBuilder<V, GlobalSync>
where
    V: OrderValidatorHandle<Order = AllOrders> + Unpin,
    GlobalSync: BlockSyncConsumer
{
    pub fn new(
        validator: V,
        order_storage: Option<Arc<OrderStorage>>,
        network_handle: StromNetworkHandle,
        eth_network_events: UnboundedReceiverStream<EthEvent>,
        order_events: UnboundedMeteredReceiver<NetworkOrderEvent>,
        global_sync: GlobalSync
    ) -> Self {
        Self {
            order_events,
            global_sync,
            eth_network_events,
            strom_network_events: network_handle.subscribe_network_events(),
            network_handle,
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
        pool_storage: AngstromPoolsTracker,
        pool_manager_tx: tokio::sync::broadcast::Sender<PoolManagerUpdate>
    ) -> PoolHandle {
        let rx = UnboundedReceiverStream::new(rx);
        let order_storage = self
            .order_storage
            .unwrap_or_else(|| Arc::new(OrderStorage::new(&self.config)));
        let handle =
            PoolHandle { manager_tx: tx.clone(), pool_manager_tx: pool_manager_tx.clone() };
        let inner = OrderIndexer::new(
            self.validator.clone(),
            order_storage.clone(),
            0,
            pool_manager_tx.clone(),
            pool_storage
        );
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
                global_sync:          self.global_sync
            })
        );

        handle
    }

    pub fn build<TP: TaskSpawner>(
        self,
        pool_storage: AngstromPoolsTracker,
        task_spawner: TP
    ) -> PoolHandle {
        let (tx, rx) = unbounded_channel();
        let rx = UnboundedReceiverStream::new(rx);
        let order_storage = self
            .order_storage
            .unwrap_or_else(|| Arc::new(OrderStorage::new(&self.config)));
        let (pool_manager_tx, _) = broadcast::channel(100);
        let handle =
            PoolHandle { manager_tx: tx.clone(), pool_manager_tx: pool_manager_tx.clone() };
        let inner = OrderIndexer::new(
            self.validator.clone(),
            order_storage.clone(),
            0,
            pool_manager_tx.clone(),
            pool_storage
        );

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
                global_sync:          self.global_sync
            })
        );

        handle
    }
}

pub struct PoolManager<V, GlobalSync>
where
    V: OrderValidatorHandle,
    GlobalSync: BlockSyncConsumer
{
    /// access to validation and sorted storage of orders.
    order_indexer:        OrderIndexer<V>,
    global_sync:          GlobalSync,
    /// Network access.
    network:              StromNetworkHandle,
    /// Subscriptions to all the strom-network related events.
    ///
    /// From which we get all new incoming order related messages.
    strom_network_events: UnboundedReceiverStream<StromNetworkEvent>,
    /// Ethereum updates stream that tells the pool manager about orders that
    /// have been filled  
    eth_network_events:   UnboundedReceiverStream<EthEvent>,
    /// receiver half of the commands to the pool manager
    command_rx:           UnboundedReceiverStream<OrderCommand>,
    /// Incoming events from the ProtocolManager.
    order_events:         UnboundedMeteredReceiver<NetworkOrderEvent>,
    /// All the connected peers.
    peer_to_info:         HashMap<PeerId, StromPeer>
}

impl<V, GlobalSync> PoolManager<V, GlobalSync>
where
    V: OrderValidatorHandle<Order = AllOrders>,
    GlobalSync: BlockSyncConsumer
{
    fn on_command(&mut self, cmd: OrderCommand) {
        match cmd {
            OrderCommand::NewOrder(_, order, validation_response) => self
                .order_indexer
                .new_rpc_order(OrderOrigin::External, order, validation_response),
            OrderCommand::CancelOrder(req, receiver) => {
                let res = self.order_indexer.cancel_order(&req);
                if res {
                    self.broadcast_cancel_to_peers(req);
                }
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

    fn on_eth_event(&mut self, eth: EthEvent, waker: Waker) {
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
                    .sign_off_reorg(MODULE_NAME, range, Some(waker))
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

    fn on_network_order_event(&mut self, event: NetworkOrderEvent) {
        match event {
            NetworkOrderEvent::IncomingOrders { peer_id, orders } => {
                orders.into_iter().for_each(|order| {
                    self.peer_to_info
                        .get_mut(&peer_id)
                        .map(|peer| peer.orders.insert(order.order_hash()));

                    self.order_indexer.new_network_order(
                        peer_id,
                        OrderOrigin::External,
                        order.clone()
                    );
                });
            }
            NetworkOrderEvent::CancelOrder { request, .. } => {
                let res = self.order_indexer.cancel_order(&request);
                if res {
                    self.broadcast_cancel_to_peers(request);
                }
            }
        }
    }

    fn on_network_event(&mut self, event: StromNetworkEvent) {
        match event {
            StromNetworkEvent::SessionEstablished { peer_id } => {
                // insert a new peer into the peerset
                self.peer_to_info.insert(
                    peer_id,
                    StromPeer {
                        orders:        LruCache::new(
                            NonZeroUsize::new(PEER_ORDER_CACHE_LIMIT).unwrap()
                        ),
                        cancellations: LruCache::new(
                            NonZeroUsize::new(PEER_ORDER_CACHE_LIMIT).unwrap()
                        )
                    }
                );
            }
            StromNetworkEvent::SessionClosed { peer_id, .. } => {
                // remove the peer
                self.peer_to_info.remove(&peer_id);
            }
            StromNetworkEvent::PeerRemoved(peer_id) => {
                self.peer_to_info.remove(&peer_id);
            }
            StromNetworkEvent::PeerAdded(peer_id) => {
                self.peer_to_info.insert(
                    peer_id,
                    StromPeer {
                        orders:        LruCache::new(
                            NonZeroUsize::new(PEER_ORDER_CACHE_LIMIT).unwrap()
                        ),
                        cancellations: LruCache::new(
                            NonZeroUsize::new(PEER_ORDER_CACHE_LIMIT).unwrap()
                        )
                    }
                );
            }
        }
    }

    fn on_pool_events(&mut self, orders: Vec<PoolInnerEvent>, waker: impl Fn() -> Waker) {
        let valid_orders = orders
            .into_iter()
            .filter_map(|order| match order {
                PoolInnerEvent::Propagation(order) => Some(order),
                PoolInnerEvent::BadOrderMessages(o) => {
                    o.into_iter().for_each(|peer| {
                        self.network.peer_reputation_change(
                            peer,
                            crate::ReputationChangeKind::InvalidOrder
                        );
                    });
                    None
                }
                PoolInnerEvent::HasTransitionedToNewBlock(block) => {
                    self.global_sync
                        .sign_off_on_block(MODULE_NAME, block, Some(waker()));
                    None
                }
                PoolInnerEvent::None => None
            })
            .collect::<Vec<_>>();

        self.broadcast_orders_to_peers(valid_orders);
    }

    fn broadcast_cancel_to_peers(&mut self, cancel: CancelOrderRequest) {
        for (peer_id, info) in self.peer_to_info.iter_mut() {
            let order_hash = cancel.order_id;
            if !info.cancellations.contains(&order_hash) {
                self.network
                    .send_message(*peer_id, StromMessage::OrderCancellation(cancel.clone()));

                info.cancellations.insert(order_hash);
            }
        }
    }

    fn broadcast_orders_to_peers(&mut self, valid_orders: Vec<AllOrders>) {
        for order in valid_orders.iter() {
            for (peer_id, info) in self.peer_to_info.iter_mut() {
                let order_hash = order.order_hash();
                if !info.orders.contains(&order_hash) {
                    self.network.send_message(
                        *peer_id,
                        StromMessage::PropagatePooledOrders(vec![order.clone()])
                    );
                    info.orders.insert(order_hash);
                }
            }
        }
    }
}

impl<V, GlobalSync> Future for PoolManager<V, GlobalSync>
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

            // drain network/peer related events
            while let Poll::Ready(Some(event)) = this.strom_network_events.poll_next_unpin(cx) {
                this.on_network_event(event);
            }

            // poll underlying pool. This is the validation process that's being polled
            while let Poll::Ready(Some(orders)) = this.order_indexer.poll_next_unpin(cx) {
                this.on_pool_events(orders, || cx.waker().clone());
            }

            // halt dealing with these till we have synced
            if this.global_sync.can_operate() {
                // drain commands
                while let Poll::Ready(Some(cmd)) = this.command_rx.poll_next_unpin(cx) {
                    this.on_command(cmd);
                    cx.waker().wake_by_ref();
                }

                // drain incoming transaction events
                while let Poll::Ready(Some(event)) = this.order_events.poll_next_unpin(cx) {
                    this.on_network_order_event(event);
                    cx.waker().wake_by_ref();
                }
            }
        }

        Poll::Pending
    }
}

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
struct StromPeer {
    /// Keeps track of transactions that we know the peer has seen.
    orders:        LruCache<B256>,
    cancellations: LruCache<B256>
}
