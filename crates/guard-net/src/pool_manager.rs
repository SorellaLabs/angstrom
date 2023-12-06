use std::{
    collections::HashMap,
    num::NonZeroUsize,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll}
};

use futures::{stream::FuturesUnordered, Future, FutureExt, StreamExt};
use guard_eth::manager::EthEvent;
use guard_types::{
    orders::{
        FromComposableLimitOrder, FromComposableSearcherOrder, FromLimitOrder, FromSearcherOrder,
        FromSignedComposableLimitOrder, FromSignedComposableSearcherOrder, FromSignedLimitOrder,
        FromSignedSearcherOrder, GetPooledOrders, OrderId, OrderLocation, OrderOrigin,
        OrderPriorityData, Orders, PooledComposableOrder, PooledLimitOrder, PooledOrder,
        PooledSearcherOrder, SearcherPriorityData, ValidatedOrder, ValidationResults
    },
    primitive::PoolId,
    rpc::*
};
use order_pool::{AllOrders, OrderPool, OrderPoolInner, OrderSet, OrdersToPropagate};
use reth_primitives::{PeerId, TxHash, B256};
use tokio::sync::{
    mpsc::{UnboundedReceiver, UnboundedSender},
    oneshot
};
use tokio_stream::wrappers::{ReceiverStream, UnboundedReceiverStream};
use validation::order::OrderValidator;

use crate::{LruCache, NetworkOrderEvent, RequestResult, StromNetworkEvent, StromNetworkHandle};
/// Cache limit of transactions to keep track of for a single peer.
const PEER_ORDER_CACHE_LIMIT: usize = 1024 * 10;

/// Api to interact with [`PoolManager`] task.
#[derive(Debug, Clone)]
pub struct PoolHandle {
    #[allow(dead_code)]
    /// Command channel to the [`TransactionsManager`]
    manager_tx: UnboundedSender<OrderCommand>
}

#[derive(Debug)]
pub enum OrderCommand {
    NewLimitOrder(OrderOrigin, EcRecoveredLimitOrder),
    NewSearcherOrder(OrderOrigin, EcRecoveredSearcherOrder),
    NewComposableLimitOrder(OrderOrigin, EcRecoveredComposableLimitOrder),
    NewComposableSearcherOrder(OrderOrigin, EcRecoveredSearcherOrder)
}

impl PoolHandle {
    #[allow(dead_code)]
    fn send(&self, cmd: OrderCommand) {
        let _ = self.manager_tx.send(cmd);
    }
}

impl OrderPool for PoolHandle {
    /// The transaction type of the composable limit order pool
    type ComposableLimitOrder = EcRecoveredComposableLimitOrder;
    /// The transaction type of the composable searcher order pool
    type ComposableSearcherOrder = EcRecoveredComposableSearcherOrder;
    /// The transaction type of the limit order pool
    type LimitOrder = EcRecoveredLimitOrder;
    /// The transaction type of the searcher order pool
    type SearcherOrder = EcRecoveredSearcherOrder;

    fn new_limit_order(&self, origin: OrderOrigin, order: Self::LimitOrder) {}

    fn new_searcher_order(&self, origin: OrderOrigin, order: Self::SearcherOrder) {}

    fn new_composable_limit_order(&self, origin: OrderOrigin, order: Self::ComposableLimitOrder) {}

    fn new_composable_searcher_order(
        &self,
        origin: OrderOrigin,
        order: Self::ComposableSearcherOrder
    ) {
    }

    async fn get_pooled_orders_by_hashes(
        &self,
        tx_hashes: Vec<TxHash>,
        limit: Option<usize>
    ) -> Vec<PooledOrder> {
        todo!()
    }

    // Queries for fetching all orders. Will be used for quoting
    // and consensus.

    // fetches all vanilla orders
    async fn get_all_vanilla_orders(&self) -> OrderSet<Self::LimitOrder, Self::SearcherOrder> {
        todo!()
    }

    // fetches all vanilla orders where for each pool the bids and asks overlap plus
    // a buffer on each side
    async fn get_all_vanilla_orders_intersection(
        &self,
        buffer: usize
    ) -> OrderSet<Self::LimitOrder, Self::SearcherOrder> {
        todo!()
    }

    async fn get_all_composable_orders(
        &self
    ) -> OrderSet<Self::ComposableLimitOrder, Self::ComposableSearcherOrder> {
        todo!()
    }

    async fn get_all_composable_orders_intersection(
        &self,
        buffer: usize
    ) -> OrderSet<Self::ComposableLimitOrder, Self::ComposableSearcherOrder> {
        todo!()
    }

    async fn get_all_orders(
        &self
    ) -> AllOrders<
        Self::LimitOrder,
        Self::SearcherOrder,
        Self::ComposableLimitOrder,
        Self::ComposableSearcherOrder
    > {
        todo!()
    }

    async fn get_all_orders_intersection(
        &self,
        buffer: usize
    ) -> AllOrders<
        Self::LimitOrder,
        Self::SearcherOrder,
        Self::ComposableLimitOrder,
        Self::ComposableSearcherOrder
    > {
        todo!()
    }
}

//TODO: Tmrw clean up + finish pool manager + pool inner
//TODO: Add metrics + events
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
    _network:             StromNetworkHandle,
    /// Subscriptions to all the strom-network related events.
    ///
    /// From which we get all new incoming order related messages.
    strom_network_events: UnboundedReceiverStream<StromNetworkEvent>,
    /// Ethereum updates stream that tells the pool manager about orders that
    /// have been filled  
    eth_network_events:   UnboundedReceiverStream<EthEvent>,
    /// Send half for the command channel. Used to generate new handles
    command_tx:           UnboundedSender<OrderCommand>,
    /// receiver half of the commands to the pool manager
    command_rx:           UnboundedReceiverStream<OrderCommand>,
    /// Order fetcher to handle inflight and missing order requests.
    _order_fetcher:       OrderFetcher,
    /// All currently pending orders grouped by peers.
    _orders_by_peers:     HashMap<TxHash, Vec<PeerId>>,
    /// Incoming events from the ProtocolManager.
    order_events:         UnboundedReceiverStream<NetworkOrderEvent>,
    /// All the connected peers.
    peers:                HashMap<PeerId, StromPeer>
}

impl<L, CL, S, CS, V> PoolManager<L, CL, S, CS, V>
where
    L: PooledLimitOrder<ValidationData = OrderPriorityData> + FromSignedLimitOrder + FromLimitOrder,
    CL: PooledComposableOrder
        + PooledLimitOrder<ValidationData = OrderPriorityData>
        + FromComposableLimitOrder
        + FromSignedComposableLimitOrder,
    S: PooledSearcherOrder<ValidationData = SearcherPriorityData>
        + FromSignedSearcherOrder
        + FromSearcherOrder,
    CS: PooledComposableOrder
        + PooledSearcherOrder<ValidationData = SearcherPriorityData>
        + FromSignedComposableSearcherOrder
        + FromComposableSearcherOrder,
    V: OrderValidator
{
    pub fn new(
        _pool: OrderPoolInner<L, CL, S, CS, V>,
        _network: StromNetworkHandle,
        _from_network: UnboundedReceiver<NetworkOrderEvent>
    ) {
        todo!()
    }
}

impl<L, CL, S, CS, V> PoolManager<L, CL, S, CS, V>
where
    L: PooledLimitOrder<ValidationData = OrderPriorityData> + FromSignedLimitOrder + FromLimitOrder,
    CL: PooledComposableOrder
        + PooledLimitOrder<ValidationData = OrderPriorityData>
        + FromComposableLimitOrder
        + FromSignedComposableLimitOrder,
    S: PooledSearcherOrder<ValidationData = SearcherPriorityData>
        + FromSignedSearcherOrder
        + FromSearcherOrder,
    CS: PooledComposableOrder
        + PooledSearcherOrder<ValidationData = SearcherPriorityData>
        + FromSignedComposableSearcherOrder
        + FromComposableSearcherOrder,
    V: OrderValidator<
        LimitOrder = L,
        SearcherOrder = S,
        ComposableLimitOrder = CL,
        ComposableSearcherOrder = CS
    >
{
    /// Returns a new handle that can send commands to this type.
    pub fn handle(&self) -> PoolHandle {
        PoolHandle { manager_tx: self.command_tx.clone() }
    }

    //TODO
    fn on_command(&mut self, cmd: OrderCommand) {}

    fn on_eth_event(&mut self, eth: EthEvent) {
        match eth {
            EthEvent::FilledOrders(orders) => {
                let orders = self.pool.filled_orders(&orders);
                todo!()
            }
            EthEvent::ReorgedOrders(orders) => {}
            EthEvent::EOAStateChanges(state_changes) => {}
        }
    }

    //TODO
    fn on_network_order_event(&mut self, event: NetworkOrderEvent) {
        match event {
            NetworkOrderEvent::IncomingOrders { peer_id, orders } => {}
        }
    }

    fn on_network_event(&mut self, event: StromNetworkEvent) {
        match event {
            StromNetworkEvent::SessionEstablished { peer_id, client_version } => {
                // insert a new peer into the peerset
                self.peers.insert(
                    peer_id,
                    StromPeer {
                        orders: LruCache::new(NonZeroUsize::new(PEER_ORDER_CACHE_LIMIT).unwrap()),
                        //request_tx: messages,
                        client_version
                    }
                );
            }
            StromNetworkEvent::SessionClosed { peer_id, .. } => {
                // remove the peer
                self.peers.remove(&peer_id);
            }

            _ => {}
        }
    }

    fn on_propagate_orders(&mut self, orders: OrdersToPropagate<L, CL, S, CS>) {
        let order = match orders {
            OrdersToPropagate::Limit(limit) => PooledOrder::Limit(limit.from_limit()),
            OrdersToPropagate::Searcher(searcher) => {
                PooledOrder::Searcher(searcher.from_searcher())
            }
            OrdersToPropagate::LimitComposable(limit) => {
                PooledOrder::ComposableLimit(limit.from_composable_limit())
            }
            OrdersToPropagate::SearcherCompsable(searcher) => {
                PooledOrder::ComposableSearcher(searcher.from_composable_searcher())
            }
        };

        // TODO: placeholder for now
        self.peers
            .values_mut()
            .for_each(|peer| peer.propagate_order(vec![order.clone()]))
    }
}

impl<L, CL, S, CS, V> Future for PoolManager<L, CL, S, CS, V>
where
    L: PooledLimitOrder<ValidationData = OrderPriorityData> + FromSignedLimitOrder + FromLimitOrder,
    CL: PooledComposableOrder
        + PooledLimitOrder<ValidationData = OrderPriorityData>
        + FromComposableLimitOrder
        + FromSignedComposableLimitOrder,
    S: PooledSearcherOrder<ValidationData = SearcherPriorityData>
        + FromSignedSearcherOrder
        + FromSearcherOrder,
    CS: PooledComposableOrder
        + PooledSearcherOrder<ValidationData = SearcherPriorityData>
        + FromSignedComposableSearcherOrder
        + FromComposableSearcherOrder,
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

        //
        while let Poll::Ready(Some(orders)) = this.pool.poll_next_unpin(cx) {
            this.on_propagate_orders(orders);
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
    IncomingOrders { peer_id: PeerId, msg: Orders },
    /// Incoming `GetPooledOrders` request from a peer.
    GetPooledOrders {
        peer_id:  PeerId,
        request:  GetPooledOrders,
        response: oneshot::Sender<RequestResult<Orders>>
    }
}

/// Tracks a single peer
#[derive(Debug)]
struct StromPeer {
    /// Keeps track of transactions that we know the peer has seen.
    #[allow(dead_code)]
    orders:         LruCache<B256>,
    /// A communication channel directly to the peer's session task.
    //request_tx:     PeerRequestSender,
    /// negotiated version of the session.
    /// The peer's client version.
    #[allow(unused)]
    client_version: Arc<str>
}

impl StromPeer {
    pub fn propagate_order(&mut self, orders: Vec<PooledOrder>) {
        todo!()
    }
}

/// The type responsible for fetching missing orders from peers.
///
/// This will keep track of unique transaction hashes that are currently being
/// fetched and submits new requests on announced hashes.
#[derive(Debug, Default)]
struct OrderFetcher {
    /// All currently active requests for pooled transactions.
    _inflight_requests:               FuturesUnordered<GetPooledOrders>,
    /// Set that tracks all hashes that are currently being fetched.
    _inflight_hash_to_fallback_peers: HashMap<TxHash, Vec<PeerId>>
}
