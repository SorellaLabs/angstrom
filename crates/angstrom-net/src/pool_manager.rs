use std::{
    collections::HashMap,
    marker::PhantomData,
    num::NonZeroUsize,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll}
};

use angstrom_eth::manager::EthEvent;
use angstrom_types::{
    orders::{OrderOrigin, OrderPriorityData, PooledOrder},
    rpc::*,
    sol_bindings::grouped_orders::{AllOrders, PoolOrder}
};
use futures::{future::BoxFuture, stream::FuturesUnordered, Future, StreamExt};
use order_pool::{OrderIndexer, OrderPoolHandle, PoolConfig, PoolInnerEvent};
use reth_metrics::common::mpsc::UnboundedMeteredReceiver;
use reth_network_peers::PeerId;
use reth_primitives::{TxHash, B256};
use reth_tasks::TaskSpawner;
use tokio::sync::{
    mpsc,
    mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
    oneshot
};
use tokio_stream::wrappers::{ReceiverStream, UnboundedReceiverStream};
use validation::order::OrderValidatorHandle;

use crate::{
    LruCache, NetworkOrderEvent, ReputationChangeKind, StromMessage, StromNetworkEvent,
    StromNetworkHandle
};

/// Cache limit of transactions to keep track of for a single peer.
const PEER_ORDER_CACHE_LIMIT: usize = 1024 * 10;

/// Api to interact with [`PoolManager`] task.
#[derive(Debug, Clone)]
pub struct PoolHandle {
    pub manager_tx: UnboundedSender<OrderCommand>
}

#[derive(Debug)]
pub enum OrderCommand {
    // new orders
    NewOrder(OrderOrigin, AllOrders)
}

impl PoolHandle {
    fn send(&self, cmd: OrderCommand) {
        let _ = self.manager_tx.send(cmd);
    }
}

impl OrderPoolHandle for PoolHandle {
    fn new_order(&self, origin: OrderOrigin, order: AllOrders) {
        self.send(OrderCommand::NewOrder(origin, order))
    }
}

pub struct PoolManagerBuilder<V>
where
    V: OrderValidatorHandle
{
    validator:            V,
    network_handle:       StromNetworkHandle,
    strom_network_events: UnboundedReceiverStream<StromNetworkEvent>,
    eth_network_events:   UnboundedReceiverStream<EthEvent>,
    order_events:         UnboundedMeteredReceiver<NetworkOrderEvent>,
    config:               PoolConfig
}

impl<V> PoolManagerBuilder<V>
where
    V: OrderValidatorHandle<Order = AllOrders> + Unpin
{
    pub fn new(
        validator: V,
        network_handle: StromNetworkHandle,
        eth_network_events: UnboundedReceiverStream<EthEvent>,
        order_events: UnboundedMeteredReceiver<NetworkOrderEvent>
    ) -> Self {
        Self {
            order_events,
            eth_network_events,
            strom_network_events: network_handle.subscribe_network_events(),
            network_handle,
            validator,
            config: Default::default()
        }
    }

    pub fn with_config(mut self, config: PoolConfig) -> Self {
        self.config = config;
        self
    }

    pub fn build_with_channels<TP: TaskSpawner>(
        self,
        task_spawner: TP,
        tx: UnboundedSender<OrderCommand>,
        rx: UnboundedReceiver<OrderCommand>
    ) -> PoolHandle {
        let rx = UnboundedReceiverStream::new(rx);
        let handle = PoolHandle { manager_tx: tx.clone() };
        let inner = OrderIndexer::new(self.validator, self.config, 0);

        task_spawner.spawn_critical(
            "transaction manager",
            Box::pin(PoolManager {
                eth_network_events:   self.eth_network_events,
                strom_network_events: self.strom_network_events,
                order_events:         self.order_events,
                peers:                HashMap::default(),
                order_sorter:         inner,
                network:              self.network_handle,
                _command_tx:          tx,
                command_rx:           rx
            })
        );

        handle
    }

    pub fn build<TP: TaskSpawner>(self, task_spawner: TP) -> PoolHandle {
        let (tx, rx) = unbounded_channel();
        let rx = UnboundedReceiverStream::new(rx);
        let handle = PoolHandle { manager_tx: tx.clone() };
        let inner = OrderIndexer::new(self.validator, self.config, 0);

        task_spawner.spawn_critical(
            "transaction manager",
            Box::pin(PoolManager {
                eth_network_events:   self.eth_network_events,
                strom_network_events: self.strom_network_events,
                order_events:         self.order_events,
                peers:                HashMap::default(),
                order_sorter:         inner,
                network:              self.network_handle,
                _command_tx:          tx,
                command_rx:           rx
            })
        );

        handle
    }
}

pub struct PoolManager<V>
where
    V: OrderValidatorHandle
{
    /// access to validation and sorted storage of orders.
    order_sorter:         OrderIndexer<V>,
    /// Network access.
    network:              StromNetworkHandle,
    /// Subscriptions to all the strom-network related events.
    ///
    /// From which we get all new incoming order related messages.
    strom_network_events: UnboundedReceiverStream<StromNetworkEvent>,
    /// Ethereum updates stream that tells the pool manager about orders that
    /// have been filled  
    eth_network_events:   UnboundedReceiverStream<EthEvent>,
    /// Send half for the command channel. Used to generate new handles
    _command_tx:          UnboundedSender<OrderCommand>,
    /// receiver half of the commands to the pool manager
    command_rx:           UnboundedReceiverStream<OrderCommand>,
    /// Incoming events from the ProtocolManager.
    order_events:         UnboundedMeteredReceiver<NetworkOrderEvent>,
    /// All the connected peers.
    peers:                HashMap<PeerId, StromPeer>
}

impl<V> PoolManager<V>
where
    V: OrderValidatorHandle<Order = AllOrders>
{
    pub fn new(
        order_sorter: OrderIndexer<V>,
        network: StromNetworkHandle,
        strom_network_events: UnboundedReceiverStream<StromNetworkEvent>,
        eth_network_events: UnboundedReceiverStream<EthEvent>,
        _command_tx: UnboundedSender<OrderCommand>,
        command_rx: UnboundedReceiverStream<OrderCommand>,
        order_events: UnboundedMeteredReceiver<NetworkOrderEvent>
    ) -> Self {
        Self {
            strom_network_events,
            network,
            order_sorter,
            peers: HashMap::new(),
            order_events,
            command_rx,
            _command_tx,
            eth_network_events
        }
    }

    fn on_command(&mut self, cmd: OrderCommand) {
        match cmd {
            OrderCommand::NewOrder(origin, order) => {}
        }
    }

    fn on_eth_event(&mut self, eth: EthEvent) {
        match eth {
            EthEvent::FilledOrders(orders, block) => {
                self.order_sorter.filled_orders(block, &orders);
            }
            EthEvent::ReorgedOrders(orders) => {
                self.order_sorter.reorg(orders);
            }
            EthEvent::EOAStateChanges(state_changes) => {
                self.order_sorter.eoa_state_change(state_changes);
            }
            EthEvent::FinalizedBlock(block) => {
                self.order_sorter.finalized_block(block);
            }
            EthEvent::NewBlock(block) => self.order_sorter.new_block(block)
        }
    }

    fn on_network_order_event(&mut self, event: NetworkOrderEvent) {
        match event {
            NetworkOrderEvent::IncomingOrders { peer_id, orders } => {
                orders.into_iter().for_each(|order| {
                    self.peers
                        .get_mut(&peer_id)
                        .map(|peer| peer.orders.insert(order.hash()));
                    // match order {
                    //     PooledOrder::Limit(order) => {
                    //         if let Ok(order) = <L as
                    // OrderConversion>::try_from_order(order) {
                    //             self.pool
                    //                 .new_limit_order(peer_id,
                    // OrderOrigin::External, order);
                    //         } else {
                    //             self.network.peer_reputation_change(
                    //                 peer_id,
                    //                 ReputationChangeKind::BadOrder
                    //             );
                    //         }
                    //     }
                    //     PooledOrder::Searcher(order) => {
                    //         if let Ok(order) = <S as
                    // OrderConversion>::try_from_order(order) {
                    //             self.pool
                    //                 .new_searcher_order(peer_id,
                    // OrderOrigin::External, order);
                    //         } else {
                    //             self.network.peer_reputation_change(
                    //                 peer_id,
                    //                 ReputationChangeKind::BadOrder
                    //             );
                    //         }
                    //     }
                    //     PooledOrder::ComposableLimit(order) => {
                    //         if let Ok(order) = <CL as
                    // OrderConversion>::try_from_order(order) {
                    //             self.pool.new_composable_limit(
                    //                 peer_id,
                    //                 OrderOrigin::External,
                    //                 order
                    //             );
                    //         } else {
                    //             self.network.peer_reputation_change(
                    //                 peer_id,
                    //                 ReputationChangeKind::BadComposableOrder
                    //             );
                    //         }
                    //     }
                    //     PooledOrder::ComposableSearcher(order) => {
                    //         if let Ok(order) = <CS as
                    // OrderConversion>::try_from_order(order) {
                    //             self.pool.new_composable_searcher_order(
                    //                 peer_id,
                    //                 OrderOrigin::External,
                    //                 order
                    //             );
                    //         } else {
                    //             self.network.peer_reputation_change(
                    //                 peer_id,
                    //                 ReputationChangeKind::BadComposableOrder
                    //             );
                    //         }
                    //     }
                    // }
                });
            }
        }
    }

    fn on_network_event(&mut self, event: StromNetworkEvent) {
        match event {
            StromNetworkEvent::SessionEstablished { peer_id } => {
                // insert a new peer into the peerset
                self.peers.insert(
                    peer_id,
                    StromPeer {
                        orders: LruCache::new(NonZeroUsize::new(PEER_ORDER_CACHE_LIMIT).unwrap())
                    }
                );
            }
            StromNetworkEvent::SessionClosed { peer_id, .. } => {
                // remove the peer
                self.peers.remove(&peer_id);
            }
            StromNetworkEvent::PeerRemoved(peer_id) => {
                self.peers.remove(&peer_id);
            }
            StromNetworkEvent::PeerAdded(peer_id) => {
                self.peers.insert(
                    peer_id,
                    StromPeer {
                        orders: LruCache::new(NonZeroUsize::new(PEER_ORDER_CACHE_LIMIT).unwrap())
                    }
                );
            }
        }
    }

    fn on_pool_events(&mut self, orders: Vec<PoolInnerEvent>) {
        let broadcast_orders = orders
            .into_iter()
            .filter_map(|order| match order {
                PoolInnerEvent::Propagation(p) => Some(p),
                PoolInnerEvent::BadOrderMessages(o) => {
                    o.into_iter().for_each(|peer| {
                        self.network.peer_reputation_change(
                            peer,
                            crate::ReputationChangeKind::InvalidOrder
                        );
                    });
                    None
                }
            })
            .collect::<Vec<_>>();

        // need to update network types for this
        // self.network
        //     .broadcast_tx(StromMessage::PropagatePooledOrders(orders))
    }
}

impl<V> Future for PoolManager<V>
where
    V: OrderValidatorHandle<Order = AllOrders> + Unpin
{
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        // pull all eth events
        while let Poll::Ready(Some(eth)) = this.eth_network_events.poll_next_unpin(cx) {
            this.on_eth_event(eth);
        }

        // drain network/peer related events
        while let Poll::Ready(Some(event)) = this.strom_network_events.poll_next_unpin(cx) {
            this.on_network_event(event);
        }

        // drain commands
        while let Poll::Ready(Some(cmd)) = this.command_rx.poll_next_unpin(cx) {
            tracing::debug!(?cmd, "that was a command");
            this.on_command(cmd);
        }

        // drain incoming transaction events
        while let Poll::Ready(Some(event)) = this.order_events.poll_next_unpin(cx) {
            this.on_network_order_event(event);
        }

        // poll underlying pool. This is the validation process that's being polled
        while let Poll::Ready(Some(orders)) = this.order_sorter.poll_next_unpin(cx) {
            this.on_pool_events(orders);
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
    IncomingOrders { peer_id: PeerId, msg: Vec<PooledOrder> }
}

/// Tracks a single peer
#[derive(Debug)]
struct StromPeer {
    /// Keeps track of transactions that we know the peer has seen.
    #[allow(dead_code)]
    orders: LruCache<B256>
}
