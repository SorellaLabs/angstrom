use std::{
    collections::HashMap,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Waker}
};

use alloy::primitives::B256;
use angstrom_eth::manager::EthEvent;
use angstrom_network::{NetworkOrderEvent, StromMessage, StromNetworkEvent, StromNetworkHandle};
use angstrom_types::{
    block_sync::BlockSyncConsumer, primitive::PeerId, sol_bindings::grouped_orders::AllOrders
};
use futures::StreamExt;
use order_pool::{OrderIndexer, PoolInnerEvent, order_storage::OrderStorage};
use reth_metrics::common::mpsc::UnboundedMeteredReceiver;
use tokio_stream::wrappers::UnboundedReceiverStream;
use validation::order::OrderValidatorHandle;

use crate::{
    MODULE_NAME, PoolManager,
    cache::LruCache,
    common::PoolManagerCommon,
    handle::{OrderCommand, PoolHandle},
    impl_common_getters, manager
};

/// Cache limit of transactions to keep track of for a single peer.
pub(crate) const PEER_ORDER_CACHE_LIMIT: usize = 1024 * 10;

/// All events related to orders emitted by the network.
#[derive(Debug)]
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

/// Consensus mode: carries networking-related state.
pub struct ConsensusMode {
    pub(crate) network:              StromNetworkHandle,
    pub(crate) strom_network_events: UnboundedReceiverStream<StromNetworkEvent>,
    pub(crate) order_events:         UnboundedMeteredReceiver<NetworkOrderEvent>,
    pub(crate) peer_to_info:         HashMap<PeerId, StromPeer>
}

pub type ConsensusPoolManager<V, GS> = PoolManager<V, GS, ConsensusMode>;

/// Builder for constructing ConsensusPoolManager instances.
pub struct ConsensusPoolManagerBuilder<V, GlobalSync>
where
    V: OrderValidatorHandle,
    GlobalSync: BlockSyncConsumer
{
    validator:            V,
    global_sync:          GlobalSync,
    order_storage:        Option<Arc<OrderStorage>>,
    network_handle:       StromNetworkHandle,
    eth_network_events:   UnboundedReceiverStream<EthEvent>,
    order_events:         UnboundedMeteredReceiver<NetworkOrderEvent>,
    strom_network_events: UnboundedReceiverStream<StromNetworkEvent>,
    config:               order_pool::PoolConfig
}

impl<V, GlobalSync> ConsensusPoolManagerBuilder<V, GlobalSync>
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
        global_sync: GlobalSync,
        strom_network_events: UnboundedReceiverStream<StromNetworkEvent>
    ) -> Self {
        Self {
            validator,
            global_sync,
            order_storage,
            network_handle,
            eth_network_events,
            order_events,
            strom_network_events,
            config: Default::default()
        }
    }

    pub fn with_config(mut self, config: order_pool::PoolConfig) -> Self {
        self.config = config;
        self
    }

    pub fn with_storage(mut self, order_storage: Arc<OrderStorage>) -> Self {
        let _ = self.order_storage.insert(order_storage);
        self
    }

    pub fn build_with_channels<TP: reth_tasks::TaskSpawner>(
        self,
        task_spawner: TP,
        tx: tokio::sync::mpsc::UnboundedSender<OrderCommand>,
        rx: tokio::sync::mpsc::UnboundedReceiver<OrderCommand>,
        pool_manager_tx: tokio::sync::broadcast::Sender<order_pool::PoolManagerUpdate>,
        block_number: u64,
        replay: impl FnOnce(&mut order_pool::OrderIndexer<V>) + Send + 'static
    ) -> PoolHandle {
        let rx = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);
        let order_storage = self
            .order_storage
            .unwrap_or_else(|| Arc::new(OrderStorage::new(&self.config)));
        let handle =
            PoolHandle { manager_tx: tx.clone(), pool_manager_tx: pool_manager_tx.clone() };
        let mut inner = order_pool::OrderIndexer::new(
            self.validator.clone(),
            order_storage.clone(),
            block_number,
            pool_manager_tx.clone()
        );
        replay(&mut inner);
        self.global_sync.register(MODULE_NAME);

        manager::spawn_manager(
            task_spawner,
            self.eth_network_events,
            inner,
            rx,
            self.global_sync,
            ConsensusMode {
                network:              self.network_handle,
                strom_network_events: self.strom_network_events,
                order_events:         self.order_events,
                peer_to_info:         HashMap::new()
            }
        );

        handle
    }
}

impl<V, GS> ConsensusPoolManager<V, GS>
where
    V: OrderValidatorHandle<Order = AllOrders> + Unpin,
    GS: BlockSyncConsumer
{
    /// Create a new consensus pool manager builder
    pub fn new(
        validator: V,
        order_storage: Option<Arc<OrderStorage>>,
        network_handle: StromNetworkHandle,
        eth_network_events: UnboundedReceiverStream<EthEvent>,
        order_events: UnboundedMeteredReceiver<NetworkOrderEvent>,
        global_sync: GS,
        strom_network_events: UnboundedReceiverStream<StromNetworkEvent>
    ) -> ConsensusPoolManagerBuilder<V, GS> {
        ConsensusPoolManagerBuilder::new(
            validator,
            order_storage,
            network_handle,
            eth_network_events,
            order_events,
            global_sync,
            strom_network_events
        )
    }
}

impl<V, GS> PoolManager<V, GS, ConsensusMode>
where
    V: OrderValidatorHandle<Order = AllOrders> + Unpin,
    GS: BlockSyncConsumer
{
    fn broadcast_cancel_to_peers(&mut self, cancel: angstrom_types::orders::CancelOrderRequest) {
        for (peer_id, info) in self.mode.peer_to_info.iter_mut() {
            let order_hash = cancel.order_id;
            if !info.cancellations.contains(&order_hash) {
                self.mode
                    .network
                    .send_message(*peer_id, StromMessage::OrderCancellation(cancel.clone()));

                info.cancellations.insert(order_hash);
            }
        }
    }

    fn broadcast_order_to_peer(&mut self, valid_orders: Vec<AllOrders>, peer: PeerId) {
        self.mode
            .network
            .send_message(peer, StromMessage::PropagatePooledOrders(valid_orders));
    }

    fn broadcast_orders_to_peers(&mut self, valid_orders: Vec<AllOrders>) {
        for order in valid_orders.iter() {
            for (peer_id, info) in self.mode.peer_to_info.iter_mut() {
                let order_hash = order.order_hash();
                if !info.orders.contains(&order_hash) {
                    self.mode.network.send_message(
                        *peer_id,
                        StromMessage::PropagatePooledOrders(vec![order.clone()])
                    );
                    info.orders.insert(order_hash);
                }
            }
        }
    }

    fn handle_pool_events(&mut self, orders: Vec<PoolInnerEvent>, waker: impl Fn() -> Waker) {
        use angstrom_types::network::ReputationChangeKind;

        let valid_orders = orders
            .into_iter()
            .filter_map(|order| match order {
                PoolInnerEvent::Propagation(order) => Some(order),
                PoolInnerEvent::BadOrderMessages(o) => {
                    o.into_iter().for_each(|peer| {
                        self.mode
                            .network
                            .peer_reputation_change(peer, ReputationChangeKind::InvalidOrder);
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

    fn on_network_order_event(&mut self, event: NetworkOrderEvent) {
        use angstrom_types::orders::OrderOrigin;
        use telemetry_recorder::telemetry_event;

        match event {
            NetworkOrderEvent::IncomingOrders { peer_id, orders } => {
                let block_num = self.global_sync.current_block_number();

                orders.into_iter().for_each(|order| {
                    self.mode
                        .peer_to_info
                        .get_mut(&peer_id)
                        .map(|peer| peer.orders.insert(order.order_hash()));

                    telemetry_event!(block_num, OrderOrigin::External, order.clone());
                    self.order_indexer.new_network_order(
                        peer_id,
                        OrderOrigin::External,
                        order.clone()
                    );
                });
            }
            NetworkOrderEvent::CancelOrder { request, .. } => {
                let block_num = self.global_sync.current_block_number();
                telemetry_recorder::telemetry_event!(block_num, request.clone());

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
                self.mode.peer_to_info.insert(
                    peer_id,
                    StromPeer {
                        orders:        LruCache::new(
                            std::num::NonZeroUsize::new(PEER_ORDER_CACHE_LIMIT).unwrap()
                        ),
                        cancellations: LruCache::new(
                            std::num::NonZeroUsize::new(PEER_ORDER_CACHE_LIMIT).unwrap()
                        )
                    }
                );
                let all_orders = self.all_orders();

                self.broadcast_order_to_peer(all_orders, peer_id);
            }
            StromNetworkEvent::SessionClosed { peer_id, .. } => {
                self.mode.peer_to_info.remove(&peer_id);
            }
            StromNetworkEvent::PeerRemoved(peer_id) => {
                self.mode.peer_to_info.remove(&peer_id);
            }
            StromNetworkEvent::PeerAdded(_) => {}
        }
    }
}

impl<V, GS> PoolManagerCommon<V, GS> for PoolManager<V, GS, ConsensusMode>
where
    V: OrderValidatorHandle<Order = AllOrders> + Unpin,
    GS: BlockSyncConsumer
{
    impl_common_getters!(PoolManager<V, GS, ConsensusMode>, V, GS);

    fn on_command(&mut self, cmd: OrderCommand) {
        match cmd {
            OrderCommand::NewOrder(origin, order, validation_response) => {
                self.handle_new_order(origin, order, validation_response)
            }
            OrderCommand::CancelOrder(req, receiver) => {
                let res = self.handle_cancel(req.clone());
                if res {
                    self.broadcast_cancel_to_peers(req);
                }
                let _ = receiver.send(res);
            }
            OrderCommand::PendingOrders(from, receiver) => {
                let _ = receiver.send(self.handle_pending_orders(from));
            }
            OrderCommand::OrderStatus(order_hash, tx) => {
                let _ = tx.send(self.handle_order_status(order_hash));
            }
            OrderCommand::OrdersByPool(pool_id, location, tx) => {
                let _ = tx.send(self.handle_orders_by_pool(pool_id, location));
            }
        }
    }

    fn on_pool_events(&mut self, orders: Vec<PoolInnerEvent>, waker: impl Fn() -> Waker) {
        self.handle_pool_events(orders, waker)
    }
}

impl<V, GS> Future for PoolManager<V, GS, ConsensusMode>
where
    V: OrderValidatorHandle<Order = AllOrders> + Unpin,
    GS: BlockSyncConsumer
{
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        use crate::common::PoolManagerCommon;
        PoolManagerCommon::poll_eth_and_pool(this, cx);

        // Medium priority: network events
        while let Poll::Ready(Some(event)) = this.mode.strom_network_events.poll_next_unpin(cx) {
            this.on_network_event(event);
        }

        // Medium priority: incoming network order events
        if this.global_sync.can_operate() {
            while let Poll::Ready(Some(event)) = this.mode.order_events.poll_next_unpin(cx) {
                this.on_network_order_event(event);
            }
        }

        // Low priority: drain commands after sync
        PoolManagerCommon::drain_commands_if_synced(this, cx);

        Poll::Pending
    }
}
