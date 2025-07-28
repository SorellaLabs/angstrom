use std::{
    num::NonZeroUsize,
    pin::Pin,
    task::{Context, Poll, Waker}
};

use angstrom_eth::manager::EthEvent;
use angstrom_types::{
    block_sync::BlockSyncConsumer,
    orders::{CancelOrderRequest, OrderOrigin},
    primitive::{NewInitializedPool, PeerId, PoolId},
    sol_bindings::grouped_orders::AllOrders
};
use futures::{Future, StreamExt};
use order_pool::PoolInnerEvent;
use telemetry_recorder::telemetry_event;
use validation::order::OrderValidatorHandle;

use crate::{
    cache::LruCache,
    order::{OrderCommand, StromPeer, PEER_ORDER_CACHE_LIMIT, MODULE_NAME, PoolManager}
};

use angstrom_types::network::{
    NetworkOrderEvent, StromNetworkEvent, ReputationChangeKind, 
    PoolStromMessage
};

/// Rollup mode for PoolManager - simpler behavior without consensus logic
#[derive(Debug)]
pub struct RollupMode;

// Ensure RollupMode can be sent between threads
unsafe impl Send for RollupMode {}
unsafe impl Sync for RollupMode {}

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

impl<V, GlobalSync> PoolManager<V, GlobalSync, RollupMode>
where
    V: OrderValidatorHandle<Order = AllOrders> + Unpin,
    GlobalSync: BlockSyncConsumer
{
    /// Rollup-specific order processing logic
    pub fn process_rollup_orders(&mut self) -> Vec<AllOrders> {
        // In rollup mode, we process all orders without consensus filtering
        // This is typically simpler and more direct
        self.order_indexer
            .get_all_orders_with_parked()
            .into_all_orders()
    }

    // Shared method implementations (duplicated for now, could be refactored to a trait later)
    fn on_command(&mut self, cmd: OrderCommand) {
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
                let all_orders = self
                    .order_indexer
                    .get_all_orders_with_parked()
                    .into_all_orders();

                self.broadcast_order_to_peer(all_orders, peer_id);
            }
            StromNetworkEvent::SessionClosed { peer_id, .. } => {
                // remove the peer
                self.peer_to_info.remove(&peer_id);
            }
            StromNetworkEvent::PeerRemoved(peer_id) => {
                self.peer_to_info.remove(&peer_id);
            }
            StromNetworkEvent::PeerAdded(_) => {}
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
                            ReputationChangeKind::InvalidOrder
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
                    .send_message(*peer_id, PoolStromMessage::OrderCancellation(cancel.clone()));

                info.cancellations.insert(order_hash);
            }
        }
    }

    fn broadcast_order_to_peer(&mut self, valid_orders: Vec<AllOrders>, peer: PeerId) {
        self.network
            .send_message(peer, PoolStromMessage::PropagatePooledOrders(valid_orders));
    }

    fn broadcast_orders_to_peers(&mut self, valid_orders: Vec<AllOrders>) {
        for order in valid_orders.iter() {
            for (peer_id, info) in self.peer_to_info.iter_mut() {
                let order_hash = order.order_hash();
                if !info.orders.contains(&order_hash) {
                    self.network.send_message(
                        *peer_id,
                        PoolStromMessage::PropagatePooledOrders(vec![order.clone()])
                    );
                    info.orders.insert(order_hash);
                }
            }
        }
    }
}

impl<V, GlobalSync> Future for PoolManager<V, GlobalSync, RollupMode>
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

            // Note: No consensus-specific polling in rollup mode
            // This makes rollup mode simpler and more focused on order processing

            // halt dealing with these till we have synced
            if this.global_sync.can_operate() {
                // drain commands
                if let Poll::Ready(Some(cmd)) = this.command_rx.poll_next_unpin(cx) {
                    this.on_command(cmd);
                    cx.waker().wake_by_ref();
                }

                // drain incoming transaction events
                if let Poll::Ready(Some(event)) = this.order_events.poll_next_unpin(cx) {
                    this.on_network_order_event(event);
                    cx.waker().wake_by_ref();
                }
            }
        }

        Poll::Pending
    }
}