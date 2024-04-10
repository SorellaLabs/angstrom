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
    orders::{
        OrderConversion, OrderOrigin, OrderPriorityData, PoolOrder, PooledComposableOrder,
        PooledLimitOrder, PooledOrder, PooledSearcherOrder, SearcherPriorityData
    },
    rpc::*
};
use futures::{future::BoxFuture, stream::FuturesUnordered, Future, StreamExt};
use order_pool::{
    AllOrders, Order, OrderPoolHandle, OrderPoolInner, OrderSet, OrdersToPropagate, PoolConfig,
    PoolInnerEvent
};
use reth_metrics::common::mpsc::UnboundedMeteredReceiver;
use reth_primitives::{PeerId, TxHash, B256};
use reth_tasks::TaskSpawner;
use tokio::sync::{
    mpsc,
    mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
    oneshot
};
use tokio_stream::wrappers::{ReceiverStream, UnboundedReceiverStream};
use validation::order::OrderValidator;

use crate::{
    LruCache, NetworkOrderEvent, ReputationChangeKind, StromMessage, StromNetworkEvent,
    StromNetworkHandle
};

/// Cache limit of transactions to keep track of for a single peer.
const PEER_ORDER_CACHE_LIMIT: usize = 1024 * 10;

/// Api to interact with [`PoolManager`] task.
#[derive(Debug, Clone)]
pub struct PoolHandle<L: PoolOrder, CL: PoolOrder, S: PoolOrder, CS: PoolOrder> {
    pub manager_tx: UnboundedSender<OrderCommand<L, CL, S, CS>>
}

#[derive(Debug)]
pub enum OrderCommand<L: PoolOrder, CL: PoolOrder, S: PoolOrder, CS: PoolOrder> {
    // new orders
    NewLimitOrder(OrderOrigin, <L as OrderConversion>::Order),
    NewSearcherOrder(OrderOrigin, <S as OrderConversion>::Order),
    NewComposableLimitOrder(OrderOrigin, <CL as OrderConversion>::Order),
    NewComposableSearcherOrder(OrderOrigin, <CS as OrderConversion>::Order),
    // fetch orders
    FetchAllVanillaOrders(oneshot::Sender<OrderSet<L, S>>, Option<usize>),
    FetchAllComposableOrders(oneshot::Sender<OrderSet<CL, CS>>, Option<usize>),
    FetchAllOrders(oneshot::Sender<AllOrders<L, S, CL, CS>>, Option<usize>),
    // subscriptions
    SubscribeNewOrders(mpsc::Sender<Order<L, CL, S, CS>>),
    SubscribeFinalizedOrders(mpsc::Sender<Vec<Order<L, CL, S, CS>>>),
    SubscribeFilledOrders(mpsc::Sender<Vec<Order<L, CL, S, CS>>>),
    SubscribeExpiredOrders(mpsc::Sender<Vec<Order<L, CL, S, CS>>>)
}

impl<L: PoolOrder, CL: PoolOrder, S: PoolOrder, CS: PoolOrder> PoolHandle<L, CL, S, CS> {
    fn send(&self, cmd: OrderCommand<L, CL, S, CS>) {
        let _ = self.manager_tx.send(cmd);
    }

    async fn send_request<T>(
        &self,
        rx: oneshot::Receiver<T>,
        cmd: OrderCommand<L, CL, S, CS>
    ) -> T {
        self.send(cmd);
        rx.await.unwrap()
    }
}

impl<L: PoolOrder, CL: PoolOrder, S: PoolOrder, CS: PoolOrder> OrderPoolHandle
    for PoolHandle<L, CL, S, CS>
{
    type ComposableLimitOrder = CL;
    type ComposableSearcherOrder = CS;
    type LimitOrder = L;
    type SearcherOrder = S;

    fn new_limit_order(
        &self,
        origin: OrderOrigin,
        order: <Self::LimitOrder as OrderConversion>::Order
    ) {
        self.send(OrderCommand::NewLimitOrder(origin, order));
    }

    fn new_searcher_order(
        &self,
        origin: OrderOrigin,
        order: <Self::SearcherOrder as OrderConversion>::Order
    ) {
        self.send(OrderCommand::NewSearcherOrder(origin, order))
    }

    fn new_composable_limit_order(
        &self,
        origin: OrderOrigin,
        order: <Self::ComposableLimitOrder as OrderConversion>::Order
    ) {
        self.send(OrderCommand::NewComposableLimitOrder(origin, order))
    }

    fn new_composable_searcher_order(
        &self,
        origin: OrderOrigin,
        order: <Self::ComposableSearcherOrder as OrderConversion>::Order
    ) {
        self.send(OrderCommand::NewComposableSearcherOrder(origin, order))
    }

    fn get_all_vanilla_orders(&self) -> BoxFuture<OrderSet<Self::LimitOrder, Self::SearcherOrder>> {
        Box::pin(async move {
            let (tx, rx) = oneshot::channel();
            self.send_request(rx, OrderCommand::FetchAllVanillaOrders(tx, None))
                .await
        })
    }

    fn get_all_vanilla_orders_intersection(
        &self,
        buffer: usize
    ) -> BoxFuture<OrderSet<Self::LimitOrder, Self::SearcherOrder>> {
        Box::pin(async move {
            let (tx, rx) = oneshot::channel();
            self.send_request(rx, OrderCommand::FetchAllVanillaOrders(tx, Some(buffer)))
                .await
        })
    }

    fn get_all_composable_orders(
        &self
    ) -> BoxFuture<OrderSet<Self::ComposableLimitOrder, Self::ComposableSearcherOrder>> {
        Box::pin(async move {
            let (tx, rx) = oneshot::channel();
            self.send_request(rx, OrderCommand::FetchAllComposableOrders(tx, None))
                .await
        })
    }

    fn get_all_composable_orders_intersection(
        &self,
        buffer: usize
    ) -> BoxFuture<OrderSet<Self::ComposableLimitOrder, Self::ComposableSearcherOrder>> {
        Box::pin(async move {
            let (tx, rx) = oneshot::channel();
            self.send_request(rx, OrderCommand::FetchAllComposableOrders(tx, Some(buffer)))
                .await
        })
    }

    fn get_all_orders(
        &self
    ) -> BoxFuture<
        AllOrders<
            Self::LimitOrder,
            Self::SearcherOrder,
            Self::ComposableLimitOrder,
            Self::ComposableSearcherOrder
        >
    > {
        Box::pin(async move {
            let (tx, rx) = oneshot::channel();
            self.send_request(rx, OrderCommand::FetchAllOrders(tx, None))
                .await
        })
    }

    fn get_all_orders_intersection(
        &self,
        buffer: usize
    ) -> BoxFuture<
        AllOrders<
            Self::LimitOrder,
            Self::SearcherOrder,
            Self::ComposableLimitOrder,
            Self::ComposableSearcherOrder
        >
    > {
        Box::pin(async move {
            let (tx, rx) = oneshot::channel();
            self.send_request(rx, OrderCommand::FetchAllOrders(tx, Some(buffer)))
                .await
        })
    }

    fn subscribe_new_orders(
        &self
    ) -> ReceiverStream<
        Order<
            Self::LimitOrder,
            Self::ComposableLimitOrder,
            Self::SearcherOrder,
            Self::ComposableSearcherOrder
        >
    > {
        let (tx, rx) = mpsc::channel(10);
        let rx = ReceiverStream::new(rx);
        self.send(OrderCommand::SubscribeNewOrders(tx));

        rx
    }

    fn subscribe_finalized_orders(
        &self
    ) -> ReceiverStream<
        Vec<
            Order<
                Self::LimitOrder,
                Self::ComposableLimitOrder,
                Self::SearcherOrder,
                Self::ComposableSearcherOrder
            >
        >
    > {
        let (tx, rx) = mpsc::channel(10);
        let rx = ReceiverStream::new(rx);
        self.send(OrderCommand::SubscribeFinalizedOrders(tx));

        rx
    }

    fn subscribe_filled_orders(
        &self
    ) -> ReceiverStream<
        Vec<
            Order<
                Self::LimitOrder,
                Self::ComposableLimitOrder,
                Self::SearcherOrder,
                Self::ComposableSearcherOrder
            >
        >
    > {
        let (tx, rx) = mpsc::channel(10);
        let rx = ReceiverStream::new(rx);
        self.send(OrderCommand::SubscribeFilledOrders(tx));

        rx
    }

    fn subscribe_expired_orders(
        &self
    ) -> ReceiverStream<
        Vec<
            Order<
                Self::LimitOrder,
                Self::ComposableLimitOrder,
                Self::SearcherOrder,
                Self::ComposableSearcherOrder
            >
        >
    > {
        let (tx, rx) = mpsc::channel(10);
        let rx = ReceiverStream::new(rx);
        self.send(OrderCommand::SubscribeExpiredOrders(tx));

        rx
    }
}

pub struct PoolManagerBuilder<L, CL, S, CS, V>
where
    L: PooledLimitOrder,
    CL: PooledComposableOrder + PooledLimitOrder,
    S: PooledSearcherOrder,
    CS: PooledComposableOrder + PooledSearcherOrder,
    V: OrderValidator
{
    validator:            V,
    network_handle:       StromNetworkHandle,
    strom_network_events: UnboundedReceiverStream<StromNetworkEvent>,
    eth_network_events:   UnboundedReceiverStream<EthEvent>,
    order_events:         UnboundedMeteredReceiver<NetworkOrderEvent>,
    config:               PoolConfig,
    _phantom:             PhantomData<(L, CL, S, CS)>
}

impl<L, CL, S, CS, V> PoolManagerBuilder<L, CL, S, CS, V>
where
    L: PooledLimitOrder<ValidationData = OrderPriorityData, Order = SignedLimitOrder>,
    CL: PooledComposableOrder
        + PooledLimitOrder<ValidationData = OrderPriorityData, Order = SignedComposableLimitOrder>,
    S: PooledSearcherOrder<ValidationData = SearcherPriorityData, Order = SignedSearcherOrder>,
    CS: PooledComposableOrder
        + PooledSearcherOrder<
            ValidationData = SearcherPriorityData,
            Order = SignedComposableSearcherOrder
        >,
    V: OrderValidator<
            LimitOrder = L,
            SearcherOrder = S,
            ComposableLimitOrder = CL,
            ComposableSearcherOrder = CS
        > + Unpin
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
            config: Default::default(),
            _phantom: Default::default()
        }
    }

    pub fn with_config(mut self, config: PoolConfig) -> Self {
        self.config = config;
        self
    }

    pub fn build_with_channels<TP: TaskSpawner>(
        self,
        task_spawner: TP,
        tx: UnboundedSender<OrderCommand<L, CL, S, CS>>,
        rx: UnboundedReceiver<OrderCommand<L, CL, S, CS>>
    ) -> PoolHandle<L, CL, S, CS> {
        let rx = UnboundedReceiverStream::new(rx);
        let handle = PoolHandle { manager_tx: tx.clone() };
        let inner = OrderPoolInner::new(self.validator, self.config);

        task_spawner.spawn_critical(
            "transaction manager",
            Box::pin(PoolManager {
                eth_network_events:   self.eth_network_events,
                strom_network_events: self.strom_network_events,
                order_events:         self.order_events,
                peers:                HashMap::default(),
                pool:                 inner,
                network:              self.network_handle,
                _command_tx:          tx,
                command_rx:           rx
            })
        );

        handle
    }

    pub fn build<TP: TaskSpawner>(self, task_spawner: TP) -> PoolHandle<L, CL, S, CS> {
        let (tx, rx) = unbounded_channel();
        let rx = UnboundedReceiverStream::new(rx);
        let handle = PoolHandle { manager_tx: tx.clone() };
        let inner = OrderPoolInner::new(self.validator, self.config);

        task_spawner.spawn_critical(
            "transaction manager",
            Box::pin(PoolManager {
                eth_network_events:   self.eth_network_events,
                strom_network_events: self.strom_network_events,
                order_events:         self.order_events,
                peers:                HashMap::default(),
                pool:                 inner,
                network:              self.network_handle,
                _command_tx:          tx,
                command_rx:           rx
            })
        );

        handle
    }
}

pub struct PoolManager<L, CL, S, CS, V>
where
    L: PooledLimitOrder,
    CL: PooledComposableOrder + PooledLimitOrder,
    S: PooledSearcherOrder,
    CS: PooledComposableOrder + PooledSearcherOrder,
    V: OrderValidator
{
    /// The order pool. Streams up new transactions to be broadcasted
    pool:                 OrderPoolInner<L, CL, S, CS, V>,
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
    _command_tx:          UnboundedSender<OrderCommand<L, CL, S, CS>>,
    /// receiver half of the commands to the pool manager
    command_rx:           UnboundedReceiverStream<OrderCommand<L, CL, S, CS>>,
    /// Incoming events from the ProtocolManager.
    order_events:         UnboundedMeteredReceiver<NetworkOrderEvent>,
    /// All the connected peers.
    peers:                HashMap<PeerId, StromPeer>
}

impl<L, CL, S, CS, V> PoolManager<L, CL, S, CS, V>
where
    L: PooledLimitOrder<ValidationData = OrderPriorityData, Order = SignedLimitOrder>,
    CL: PooledComposableOrder
        + PooledLimitOrder<ValidationData = OrderPriorityData, Order = SignedComposableLimitOrder>,
    S: PooledSearcherOrder<ValidationData = SearcherPriorityData, Order = SignedSearcherOrder>,
    CS: PooledComposableOrder
        + PooledSearcherOrder<
            ValidationData = SearcherPriorityData,
            Order = SignedComposableSearcherOrder
        >,
    V: OrderValidator<
        LimitOrder = L,
        SearcherOrder = S,
        ComposableLimitOrder = CL,
        ComposableSearcherOrder = CS
    >
{
    pub fn new(
        pool: OrderPoolInner<L, CL, S, CS, V>,
        network: StromNetworkHandle,
        strom_network_events: UnboundedReceiverStream<StromNetworkEvent>,
        eth_network_events: UnboundedReceiverStream<EthEvent>,
        _command_tx: UnboundedSender<OrderCommand<L, CL, S, CS>>,
        command_rx: UnboundedReceiverStream<OrderCommand<L, CL, S, CS>>,
        order_events: UnboundedMeteredReceiver<NetworkOrderEvent>
    ) -> Self {
        Self {
            strom_network_events,
            network,
            pool,
            peers: HashMap::new(),
            order_events,
            command_rx,
            _command_tx,
            eth_network_events
        }
    }

    fn on_command(&mut self, cmd: OrderCommand<L, CL, S, CS>) {
        match cmd {
            // new orders
            OrderCommand::NewLimitOrder(origin, order) => {
                if let Ok(order) = <L as OrderConversion>::try_from_order(order) {
                    self.pool.new_limit_order(PeerId::ZERO, origin, order);
                }
            }
            OrderCommand::NewSearcherOrder(origin, order) => {
                if let Ok(order) = <S as OrderConversion>::try_from_order(order) {
                    self.pool.new_searcher_order(PeerId::ZERO, origin, order);
                }
            }
            OrderCommand::NewComposableLimitOrder(origin, order) => {
                if let Ok(order) = <CL as OrderConversion>::try_from_order(order) {
                    self.pool.new_composable_limit(PeerId::ZERO, origin, order);
                }
            }
            OrderCommand::NewComposableSearcherOrder(origin, order) => {
                if let Ok(order) = <CS as OrderConversion>::try_from_order(order) {
                    self.pool
                        .new_composable_searcher_order(PeerId::ZERO, origin, order);
                }
            }
            // fetch requests
            OrderCommand::FetchAllOrders(sender, is_intersection) => {
                if let Some(_buffer) = is_intersection {
                    todo!()
                } else {
                    let vanilla = self.pool.fetch_vanilla_orders();
                    let composable = self.pool.fetch_composable_orders();

                    let _ = sender.send(AllOrders { vanilla, composable });
                }
            }
            OrderCommand::FetchAllComposableOrders(sender, is_intersection) => {
                if let Some(_buffer) = is_intersection {
                    todo!()
                } else {
                    let _ = sender.send(self.pool.fetch_composable_orders());
                }
            }
            OrderCommand::FetchAllVanillaOrders(sender, is_intersection) => {
                if let Some(_buffer) = is_intersection {
                    todo!()
                } else {
                    let _ = sender.send(self.pool.fetch_vanilla_orders());
                }
            }
            OrderCommand::SubscribeNewOrders(tx) => self.pool.subscribe_new_orders(tx),
            OrderCommand::SubscribeFilledOrders(tx) => self.pool.subscribe_filled_orders(tx),
            OrderCommand::SubscribeExpiredOrders(tx) => self.pool.subscribe_expired_orders(tx),
            OrderCommand::SubscribeFinalizedOrders(tx) => self.pool.subscribe_finalized_orders(tx)
        }
    }

    fn on_eth_event(&mut self, eth: EthEvent) {
        match eth {
            EthEvent::FilledOrders(orders, block) => {
                self.pool.filled_orders(block, &orders);
            }
            EthEvent::ReorgedOrders(orders) => self.pool.reorg(orders),
            EthEvent::EOAStateChanges(state_changes) => {
                self.pool.eoa_state_change(state_changes);
            }
            EthEvent::FinalizedBlock(block) => {
                self.pool.finalized_block(block);
            }
            EthEvent::NewBlock => self.pool.new_block()
        }
    }

    fn on_network_order_event(&mut self, event: NetworkOrderEvent) {
        match event {
            NetworkOrderEvent::IncomingOrders { peer_id, orders } => {
                orders.into_iter().for_each(|order| {
                    self.peers
                        .get_mut(&peer_id)
                        .map(|peer| peer.orders.insert(order.hash()));

                    match order {
                        PooledOrder::Limit(order) => {
                            if let Ok(order) = <L as OrderConversion>::try_from_order(order) {
                                self.pool
                                    .new_limit_order(peer_id, OrderOrigin::External, order);
                            } else {
                                self.network.peer_reputation_change(
                                    peer_id,
                                    ReputationChangeKind::BadOrder
                                );
                            }
                        }
                        PooledOrder::Searcher(order) => {
                            if let Ok(order) = <S as OrderConversion>::try_from_order(order) {
                                self.pool
                                    .new_searcher_order(peer_id, OrderOrigin::External, order);
                            } else {
                                self.network.peer_reputation_change(
                                    peer_id,
                                    ReputationChangeKind::BadOrder
                                );
                            }
                        }
                        PooledOrder::ComposableLimit(order) => {
                            if let Ok(order) = <CL as OrderConversion>::try_from_order(order) {
                                self.pool.new_composable_limit(
                                    peer_id,
                                    OrderOrigin::External,
                                    order
                                );
                            } else {
                                self.network.peer_reputation_change(
                                    peer_id,
                                    ReputationChangeKind::BadComposableOrder
                                );
                            }
                        }
                        PooledOrder::ComposableSearcher(order) => {
                            if let Ok(order) = <CS as OrderConversion>::try_from_order(order) {
                                self.pool.new_composable_searcher_order(
                                    peer_id,
                                    OrderOrigin::External,
                                    order
                                );
                            } else {
                                self.network.peer_reputation_change(
                                    peer_id,
                                    ReputationChangeKind::BadComposableOrder
                                );
                            }
                        }
                    }
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

    fn on_pool_events(&mut self, orders: Vec<PoolInnerEvent<L, CL, S, CS>>) {
        let orders = orders
            .into_iter()
            .filter_map(|order| match order {
                PoolInnerEvent::Propagation(p) => Some(p.into_pooled()),
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

        self.network
            .broadcast_tx(StromMessage::PropagatePooledOrders(orders))
    }
}

impl<L, CL, S, CS, V> Future for PoolManager<L, CL, S, CS, V>
where
    L: PooledLimitOrder<ValidationData = OrderPriorityData, Order = SignedLimitOrder>,
    CL: PooledComposableOrder
        + PooledLimitOrder<ValidationData = OrderPriorityData, Order = SignedComposableLimitOrder>,
    S: PooledSearcherOrder<ValidationData = SearcherPriorityData, Order = SignedSearcherOrder>,
    CS: PooledComposableOrder
        + PooledSearcherOrder<
            ValidationData = SearcherPriorityData,
            Order = SignedComposableSearcherOrder
        >,
    V: OrderValidator<
            LimitOrder = L,
            SearcherOrder = S,
            ComposableLimitOrder = CL,
            ComposableSearcherOrder = CS
        > + Unpin
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
            this.on_command(cmd);
        }

        // drain incoming transaction events
        while let Poll::Ready(Some(event)) = this.order_events.poll_next_unpin(cx) {
            this.on_network_order_event(event);
        }

        // poll underlying pool. This is the validation process that's being polled
        while let Poll::Ready(Some(orders)) = this.pool.poll_next_unpin(cx) {
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
