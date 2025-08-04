use std::{
    collections::HashMap,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll}
};

use angstrom_eth::manager::EthEvent;
use angstrom_network::{NetworkHandle, NetworkOrderEvent, StromNetworkEvent};
use angstrom_types::{
    block_sync::BlockSyncConsumer, primitive::PeerId, sol_bindings::grouped_orders::AllOrders
};
use futures::{Future, StreamExt};
use order_pool::{OrderIndexer, PoolInnerEvent, order_storage::OrderStorage};
use reth_metrics::common::mpsc::UnboundedMeteredReceiver;
use tokio_stream::wrappers::UnboundedReceiverStream;
use validation::order::OrderValidatorHandle;

use crate::{
    common::PoolManagerCommon,
    impl_common_getters,
    order::{MODULE_NAME, OrderCommand, PoolHandle, StromPeer}
};

/// Consensus-mode pool manager with full networking capabilities
pub struct ConsensusPoolManager<V, GS, NH>
where
    V: OrderValidatorHandle,
    GS: BlockSyncConsumer,
    NH: NetworkHandle
{
    /// Access to validation and sorted storage of orders
    pub(crate) order_indexer:        OrderIndexer<V>,
    /// Global blockchain synchronization coordinator
    pub(crate) global_sync:          GS,
    /// Network access for peer communication
    pub(crate) network:              NH,
    /// Ethereum updates stream that tells the pool manager about orders that
    /// have been filled
    pub(crate) eth_network_events:   UnboundedReceiverStream<EthEvent>,
    /// Receiver half of the commands to the pool manager
    pub(crate) command_rx:           UnboundedReceiverStream<OrderCommand>,
    /// Subscriptions to all the strom-network related events.
    /// From which we get all new incoming order related messages.
    pub(crate) strom_network_events: UnboundedReceiverStream<StromNetworkEvent>,
    /// Incoming events from the ProtocolManager.
    pub(crate) order_events:         UnboundedMeteredReceiver<NetworkOrderEvent>,
    /// All the connected peers.
    pub(crate) peer_to_info:         HashMap<PeerId, StromPeer>
}

/// Builder for constructing ConsensusPoolManager instances.
pub struct ConsensusPoolManagerBuilder<V, GlobalSync, NH>
where
    V: OrderValidatorHandle,
    GlobalSync: BlockSyncConsumer,
    NH: NetworkHandle
{
    validator:            V,
    global_sync:          GlobalSync,
    order_storage:        Option<Arc<OrderStorage>>,
    network_handle:       NH,
    eth_network_events:   UnboundedReceiverStream<EthEvent>,
    order_events:         UnboundedMeteredReceiver<NetworkOrderEvent>,
    strom_network_events: UnboundedReceiverStream<StromNetworkEvent>,
    config:               order_pool::PoolConfig
}

impl<V, GlobalSync, NH> ConsensusPoolManagerBuilder<V, GlobalSync, NH>
where
    V: OrderValidatorHandle<Order = AllOrders> + Unpin,
    GlobalSync: BlockSyncConsumer,
    NH: NetworkHandle<Events<'static> = UnboundedReceiverStream<StromNetworkEvent>>
        + Send
        + Sync
        + Unpin
        + 'static
{
    pub fn new(
        validator: V,
        order_storage: Option<Arc<OrderStorage>>,
        network_handle: NH,
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
        tx: tokio::sync::mpsc::UnboundedSender<crate::order::OrderCommand>,
        rx: tokio::sync::mpsc::UnboundedReceiver<crate::order::OrderCommand>,
        pool_manager_tx: tokio::sync::broadcast::Sender<order_pool::PoolManagerUpdate>,
        block_number: u64,
        replay: impl FnOnce(&mut order_pool::OrderIndexer<V>) + Send + 'static
    ) -> crate::order::PoolHandle {
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

        task_spawner.spawn_critical(
            "order pool manager",
            Box::pin(ConsensusPoolManager {
                eth_network_events:   self.eth_network_events,
                order_indexer:        inner,
                network:              self.network_handle,
                command_rx:           rx,
                global_sync:          self.global_sync,
                strom_network_events: self.strom_network_events,
                order_events:         self.order_events,
                peer_to_info:         HashMap::new()
            })
        );

        handle
    }
}

impl<V, GS, NH> ConsensusPoolManager<V, GS, NH>
where
    V: OrderValidatorHandle<Order = AllOrders> + Unpin,
    GS: BlockSyncConsumer,
    NH: NetworkHandle<Events<'static> = UnboundedReceiverStream<StromNetworkEvent>>
        + Send
        + Sync
        + Unpin
        + 'static
{
    /// Create a new consensus pool manager builder
    pub fn new(
        validator: V,
        order_storage: Option<Arc<OrderStorage>>,
        network_handle: NH,
        eth_network_events: UnboundedReceiverStream<EthEvent>,
        order_events: UnboundedMeteredReceiver<NetworkOrderEvent>,
        global_sync: GS,
        strom_network_events: UnboundedReceiverStream<StromNetworkEvent>
    ) -> ConsensusPoolManagerBuilder<V, GS, NH> {
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

impl<V, GlobalSync, NH> ConsensusPoolManager<V, GlobalSync, NH>
where
    V: OrderValidatorHandle<Order = AllOrders> + Unpin,
    GlobalSync: BlockSyncConsumer,
    NH: NetworkHandle
{
    /// Consensus-specific order processing logic
    ///
    /// This method provides a convenient way to get proposable orders
    /// that respects the consensus mode's filtering logic.
    pub fn get_consensus_orders(&mut self) -> Vec<AllOrders> {
        // In consensus mode, we might need to filter orders based on consensus state
        // or apply consensus-specific validation rules
        self.order_indexer
            .get_all_orders_with_parked()
            .into_all_orders()
    }

    /// Handle incoming network order events - consensus mode specific
    pub(crate) fn on_network_order_event(&mut self, event: NetworkOrderEvent) {
        use angstrom_types::orders::OrderOrigin;
        use telemetry_recorder::telemetry_event;

        match event {
            NetworkOrderEvent::IncomingOrders { peer_id, orders } => {
                let block_num = self.global_sync.current_block_number();

                orders.into_iter().for_each(|order| {
                    self.peer_to_info
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
                telemetry_event!(block_num, request.clone());

                let res = self.order_indexer.cancel_order(&request);
                if res {
                    self.broadcast_cancel_to_peers(request);
                }
            }
        }
    }

    /// Handle network peer events - consensus mode specific
    pub(crate) fn on_network_event(&mut self, event: StromNetworkEvent) {
        use std::num::NonZeroUsize;

        use crate::{cache::LruCache, order::PEER_ORDER_CACHE_LIMIT};

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
                let all_orders = self.get_consensus_orders();

                self.broadcast_order_to_peer(all_orders, peer_id);
            }
            StromNetworkEvent::SessionClosed { peer_id, .. } => {
                self.peer_to_info.remove(&peer_id);
            }
            StromNetworkEvent::PeerRemoved(peer_id) => {
                self.peer_to_info.remove(&peer_id);
            }
            StromNetworkEvent::PeerAdded(_) => {}
        }
    }

    fn broadcast_cancel_to_peers(&mut self, cancel: angstrom_types::orders::CancelOrderRequest) {
        use angstrom_types::network::PoolNetworkMessage;

        for (peer_id, info) in self.peer_to_info.iter_mut() {
            let order_hash = cancel.order_id;
            if !info.cancellations.contains(&order_hash) {
                self.network
                    .send_message(*peer_id, PoolNetworkMessage::OrderCancellation(cancel.clone()));

                info.cancellations.insert(order_hash);
            }
        }
    }

    fn broadcast_order_to_peer(&mut self, valid_orders: Vec<AllOrders>, peer: PeerId) {
        use angstrom_types::network::PoolNetworkMessage;

        self.network
            .send_message(peer, PoolNetworkMessage::PropagatePooledOrders(valid_orders));
    }

    fn broadcast_orders_to_peers(&mut self, valid_orders: Vec<AllOrders>) {
        use angstrom_types::network::PoolNetworkMessage;

        for order in valid_orders.iter() {
            for (peer_id, info) in self.peer_to_info.iter_mut() {
                let order_hash = order.order_hash();
                if !info.orders.contains(&order_hash) {
                    self.network.send_message(
                        *peer_id,
                        PoolNetworkMessage::PropagatePooledOrders(vec![order.clone()])
                    );
                    info.orders.insert(order_hash);
                }
            }
        }
    }

    fn on_pool_events(
        &mut self,
        orders: Vec<PoolInnerEvent>,
        waker: impl Fn() -> std::task::Waker
    ) {
        use angstrom_types::network::ReputationChangeKind;

        let valid_orders = orders
            .into_iter()
            .filter_map(|order| match order {
                PoolInnerEvent::Propagation(order) => Some(order),
                PoolInnerEvent::BadOrderMessages(o) => {
                    o.into_iter().for_each(|peer| {
                        self.network
                            .peer_reputation_change(peer, ReputationChangeKind::InvalidOrder);
                    });
                    None
                }
                PoolInnerEvent::HasTransitionedToNewBlock(block) => {
                    self.global_sync.sign_off_on_block(
                        crate::order::MODULE_NAME,
                        block,
                        Some(waker())
                    );
                    None
                }
                PoolInnerEvent::None => None
            })
            .collect::<Vec<_>>();

        self.broadcast_orders_to_peers(valid_orders);
    }
}

// Implement the PoolManagerCommon trait methods
impl<V, GS, NH> PoolManagerCommon for ConsensusPoolManager<V, GS, NH>
where
    V: OrderValidatorHandle<Order = AllOrders> + Unpin,
    GS: BlockSyncConsumer,
    NH: NetworkHandle
{
    type GlobalSync = GS;
    type Validator = V;

    // Use macro to avoid duplication of getter methods
    impl_common_getters!(ConsensusPoolManager<V, GS, NH>, V, GS);

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

    fn on_pool_events(
        &mut self,
        orders: Vec<PoolInnerEvent>,
        waker: impl Fn() -> std::task::Waker
    ) {
        use angstrom_types::network::ReputationChangeKind;

        let valid_orders = orders
            .into_iter()
            .filter_map(|order| match order {
                PoolInnerEvent::Propagation(order) => Some(order),
                PoolInnerEvent::BadOrderMessages(o) => {
                    o.into_iter().for_each(|peer| {
                        self.network
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
}

impl<V, GlobalSync, NH> Future for ConsensusPoolManager<V, GlobalSync, NH>
where
    V: OrderValidatorHandle<Order = AllOrders> + Unpin,
    GlobalSync: BlockSyncConsumer,
    NH: NetworkHandle<Events<'static> = UnboundedReceiverStream<StromNetworkEvent>>
        + Unpin
        + 'static
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

            // Poll mode-specific events
            // Poll network/peer related events - consensus mode specific
            while let std::task::Poll::Ready(Some(event)) =
                this.strom_network_events.poll_next_unpin(cx)
            {
                this.on_network_event(event);
            }

            // Poll incoming network order events - consensus mode specific
            if this.global_sync.can_operate() {
                if let std::task::Poll::Ready(Some(event)) = this.order_events.poll_next_unpin(cx) {
                    this.on_network_order_event(event);
                    cx.waker().wake_by_ref();
                }
            }

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
