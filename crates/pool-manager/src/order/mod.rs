use alloy::primitives::{Address, B256, FixedBytes};
use angstrom_eth::manager::EthEvent;
use angstrom_network::NetworkHandle;
use angstrom_types::{
    block_sync::BlockSyncConsumer,
    orders::{CancelOrderRequest, OrderLocation, OrderOrigin, OrderStatus},
    primitive::{OrderValidationError, PeerId},
    sol_bindings::grouped_orders::AllOrders
};
use futures::{Future, FutureExt};
use order_pool::{OrderIndexer, OrderPoolHandle, PoolManagerUpdate};
use tokio::sync::mpsc::{UnboundedSender, error::SendError};
use tokio_stream::wrappers::{BroadcastStream, UnboundedReceiverStream};
use validation::order::{OrderValidationResults, OrderValidatorHandle};

use crate::{PoolManagerMode, cache::LruCache};

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

pub struct PoolManager<V, GlobalSync, NH: NetworkHandle, M = crate::consensus::ConsensusMode>
where
    V: OrderValidatorHandle,
    GlobalSync: BlockSyncConsumer,
    M: PoolManagerMode
{
    /// access to validation and sorted storage of orders.
    pub(crate) order_indexer:      OrderIndexer<V>,
    pub(crate) global_sync:        GlobalSync,
    /// Network access.
    pub(crate) network:            NH,
    /// Ethereum updates stream that tells the pool manager about orders that
    /// have been filled
    pub(crate) eth_network_events: UnboundedReceiverStream<EthEvent>,
    /// receiver half of the commands to the pool manager
    pub(crate) command_rx:         UnboundedReceiverStream<OrderCommand>,
    /// Mode-specific state and behavior
    pub(crate) mode:               M
}

// All pool manager implementation methods are now mode-specific.
// See consensus.rs and rollup.rs for the specific implementations.

// Future implementations are now mode-specific - see consensus.rs and rollup.rs

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

// Type aliases are now available in the crate root (lib.rs) for convenience

// Mode-specific constructor implementations
