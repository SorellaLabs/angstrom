//! Generic PoolManager with type-state modes.
//!
//! Mirrors the pattern used by `amm-quoter`: a single generic manager
//! struct parameterized by a mode type that carries mode-specific state
//! and behavior.

use std::{collections::HashMap, pin::Pin, task::{Context, Poll, Waker}};

use angstrom_eth::manager::EthEvent;
use angstrom_network::{NetworkHandle, NetworkOrderEvent, StromNetworkEvent};
use angstrom_types::{
    block_sync::BlockSyncConsumer,
    primitive::PeerId,
    sol_bindings::grouped_orders::AllOrders,
};
use futures::{Future, StreamExt};
use order_pool::{OrderIndexer, PoolInnerEvent};
use reth_metrics::common::mpsc::UnboundedMeteredReceiver;
use tokio_stream::wrappers::UnboundedReceiverStream;
use validation::order::OrderValidatorHandle;

use crate::{
    common::PoolManagerCommon,
    impl_common_getters,
    order::{MODULE_NAME, OrderCommand, StromPeer},
};

/// Shared, generic PoolManager state.
pub struct PoolManager<V, GS, M>
where
    V: OrderValidatorHandle<Order = AllOrders> + Unpin,
    GS: BlockSyncConsumer,
{
    pub(crate) order_indexer: OrderIndexer<V>,
    pub(crate) global_sync: GS,
    pub(crate) eth_network_events: UnboundedReceiverStream<EthEvent>,
    pub(crate) command_rx: UnboundedReceiverStream<OrderCommand>,
    pub(crate) mode: M,
}

/// Rollup mode: no networking state.
#[derive(Debug, Default, Clone, Copy)]
pub struct RollupMode;

/// Consensus mode: carries networking-related state.
pub struct ConsensusMode<NH>
where
    NH: NetworkHandle,
{
    pub(crate) network: NH,
    pub(crate) strom_network_events: UnboundedReceiverStream<StromNetworkEvent>,
    pub(crate) order_events: UnboundedMeteredReceiver<NetworkOrderEvent>,
    pub(crate) peer_to_info: HashMap<PeerId, StromPeer>,
}

// Public type aliases to mirror previous concrete types
pub type RollupPoolManager<V, GS> = PoolManager<V, GS, RollupMode>;
pub type ConsensusPoolManager<V, GS, NH> = PoolManager<V, GS, ConsensusMode<NH>>;

impl<V, GS> PoolManager<V, GS, RollupMode>
where
    V: OrderValidatorHandle<Order = AllOrders> + Unpin,
    GS: BlockSyncConsumer,
{
    /// Rollup-specific helper to fetch orders; currently identical to consensus,
    /// but kept here to mirror the previous API surface.
    pub fn get_rollup_orders(&mut self) -> Vec<AllOrders> {
        self.order_indexer
            .get_all_orders_with_parked()
            .into_all_orders()
    }
}

impl<V, GS, NH> PoolManager<V, GS, ConsensusMode<NH>>
where
    V: OrderValidatorHandle<Order = AllOrders> + Unpin,
    GS: BlockSyncConsumer,
    NH: NetworkHandle,
{
    /// Consensus-specific helper to fetch orders; may apply filtering later.
    pub fn get_consensus_orders(&mut self) -> Vec<AllOrders> {
        self.order_indexer
            .get_all_orders_with_parked()
            .into_all_orders()
    }

    fn broadcast_cancel_to_peers(
        &mut self,
        cancel: angstrom_types::orders::CancelOrderRequest,
    ) {
        use angstrom_types::network::PoolNetworkMessage;

        for (peer_id, info) in self.mode.peer_to_info.iter_mut() {
            let order_hash = cancel.order_id;
            if !info.cancellations.contains(&order_hash) {
                self.mode
                    .network
                    .send_message(*peer_id, PoolNetworkMessage::OrderCancellation(cancel.clone()));

                info.cancellations.insert(order_hash);
            }
        }
    }

    fn broadcast_order_to_peer(&mut self, valid_orders: Vec<AllOrders>, peer: PeerId) {
        use angstrom_types::network::PoolNetworkMessage;

        self.mode
            .network
            .send_message(peer, PoolNetworkMessage::PropagatePooledOrders(valid_orders));
    }

    fn broadcast_orders_to_peers(&mut self, valid_orders: Vec<AllOrders>) {
        use angstrom_types::network::PoolNetworkMessage;

        for order in valid_orders.iter() {
            for (peer_id, info) in self.mode.peer_to_info.iter_mut() {
                let order_hash = order.order_hash();
                if !info.orders.contains(&order_hash) {
                    self.mode.network.send_message(
                        *peer_id,
                        PoolNetworkMessage::PropagatePooledOrders(vec![order.clone()]),
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
                PoolInnerEvent::None => None,
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
                    self.order_indexer
                        .new_network_order(peer_id, OrderOrigin::External, order.clone());
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
                        orders:        crate::cache::LruCache::new(
                            std::num::NonZeroUsize::new(crate::PEER_ORDER_CACHE_LIMIT).unwrap(),
                        ),
                        cancellations: crate::cache::LruCache::new(
                            std::num::NonZeroUsize::new(crate::PEER_ORDER_CACHE_LIMIT).unwrap(),
                        ),
                    },
                );
                let all_orders = self.get_consensus_orders();

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

// Implement common getters and shared eth-event handler via PoolManagerCommon
impl<V, GS> PoolManagerCommon for PoolManager<V, GS, RollupMode>
where
    V: OrderValidatorHandle<Order = AllOrders> + Unpin,
    GS: BlockSyncConsumer,
{
    type GlobalSync = GS;
    type Validator = V;

    impl_common_getters!(PoolManager<V, GS, RollupMode>, V, GS);

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
                // rollup: no broadcast
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

    fn on_pool_events(&mut self, orders: Vec<PoolInnerEvent>, waker: impl Fn() -> Waker) {
        for order in orders {
            match order {
                PoolInnerEvent::Propagation(_order) => {
                    // no-op in rollup
                }
                PoolInnerEvent::BadOrderMessages(_o) => {
                    // no networking in rollup
                }
                PoolInnerEvent::HasTransitionedToNewBlock(block) => {
                    self.global_sync
                        .sign_off_on_block(MODULE_NAME, block, Some(waker()));
                }
                PoolInnerEvent::None => {}
            }
        }
    }
}

impl<V, GS, NH> PoolManagerCommon for PoolManager<V, GS, ConsensusMode<NH>>
where
    V: OrderValidatorHandle<Order = AllOrders> + Unpin,
    GS: BlockSyncConsumer,
    NH: NetworkHandle,
{
    type GlobalSync = GS;
    type Validator = V;

    impl_common_getters!(PoolManager<V, GS, ConsensusMode<NH>>, V, GS);

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

    fn on_pool_events(&mut self, orders: Vec<PoolInnerEvent>, waker: impl Fn() -> Waker) {
        self.handle_pool_events(orders, waker)
    }
}

impl<V, GS> Future for PoolManager<V, GS, RollupMode>
where
    V: OrderValidatorHandle<Order = AllOrders> + Unpin,
    GS: BlockSyncConsumer,
{
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        // High priority: pull all eth events
        while let Poll::Ready(Some(eth)) = this.eth_network_events.poll_next_unpin(cx) {
            this.on_eth_event(eth, cx.waker().clone());
        }

        // High priority: poll underlying pool (validation process)
        while let Poll::Ready(Some(orders)) = this.order_indexer.poll_next_unpin(cx) {
            this.on_pool_events(orders, || cx.waker().clone());
        }

        // Low priority: drain commands after sync
        if this.global_sync.can_operate() {
            while let Poll::Ready(Some(cmd)) = this.command_rx.poll_next_unpin(cx) {
                this.on_command(cmd);
            }
        }

        Poll::Pending
    }
}

impl<V, GS, NH> Future for PoolManager<V, GS, ConsensusMode<NH>>
where
    V: OrderValidatorHandle<Order = AllOrders> + Unpin,
    GS: BlockSyncConsumer,
    NH: NetworkHandle + Unpin + 'static,
{
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        // High priority: pull all eth events
        while let Poll::Ready(Some(eth)) = this.eth_network_events.poll_next_unpin(cx) {
            this.on_eth_event(eth, cx.waker().clone());
        }

        // High priority: poll underlying pool (validation process)
        while let Poll::Ready(Some(orders)) = this.order_indexer.poll_next_unpin(cx) {
            this.on_pool_events(orders, || cx.waker().clone());
        }

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
        if this.global_sync.can_operate() {
            while let Poll::Ready(Some(cmd)) = this.command_rx.poll_next_unpin(cx) {
                this.on_command(cmd);
            }
        }

        Poll::Pending
    }
}
