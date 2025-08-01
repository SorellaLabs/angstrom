use std::{
    sync::Arc,
    task::{Context, Poll}
};

use angstrom_eth::manager::EthEvent;
use angstrom_network::{NetworkHandle, NetworkOrderEvent, StromNetworkEvent};
use angstrom_types::{block_sync::BlockSyncConsumer, sol_bindings::grouped_orders::AllOrders};
use futures::StreamExt;
use order_pool::order_storage::OrderStorage;
use reth_metrics::common::mpsc::UnboundedMeteredReceiver;
use tokio_stream::wrappers::UnboundedReceiverStream;
use validation::order::OrderValidatorHandle;

use crate::order::{PoolManager, PoolManagerMode};

/// A type alias for the consensus pool manager.
pub type ConsensusPoolManager<V, GS, NH> = PoolManager<V, GS, NH, ConsensusMode>;

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
    ) -> crate::order::PoolManagerBuilder<V, GS, NH, ConsensusMode> {
        crate::order::PoolManagerBuilder::new(
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

/// Consensus mode for PoolManager - includes consensus-specific state and
/// behavior
#[derive(Debug)]
pub struct ConsensusMode {
    /// Consensus-specific state could be added here in the future
    /// For example: consensus streams, pre-proposal tracking, etc.
    _consensus_state: ()
}

impl ConsensusMode {
    pub fn new() -> Self {
        Self { _consensus_state: () }
    }
}

impl Default for ConsensusMode {
    fn default() -> Self {
        Self::new()
    }
}

impl PoolManagerMode for ConsensusMode {
    const REQUIRES_NETWORKING: bool = true;

    fn get_proposable_orders<V, GS, NH>(pool: &mut PoolManager<V, GS, NH, Self>) -> Vec<AllOrders>
    where
        V: OrderValidatorHandle<Order = AllOrders> + Unpin,
        GS: BlockSyncConsumer,
        NH: NetworkHandle,
        Self: Sized
    {
        // In consensus mode, we might need to filter orders based on consensus state
        // or apply consensus-specific validation rules
        pool.order_indexer
            .get_all_orders_with_parked()
            .into_all_orders()
    }

    fn poll_mode_specific<V, GS, NH>(pool: &mut PoolManager<V, GS, NH, Self>, cx: &mut Context<'_>)
    where
        V: OrderValidatorHandle<Order = AllOrders> + Unpin,
        GS: BlockSyncConsumer,
        NH: NetworkHandle<Events<'static> = UnboundedReceiverStream<StromNetworkEvent>> + 'static,
        Self: Sized
    {
        // Poll network/peer related events - only in consensus mode
        while let Poll::Ready(Some(event)) = pool.strom_network_events.poll_next_unpin(cx) {
            pool.on_network_event(event);
        }

        // Poll incoming network order events - only in consensus mode
        if pool.global_sync.can_operate() {
            if let Poll::Ready(Some(event)) = pool.order_events.poll_next_unpin(cx) {
                pool.on_network_order_event(event);
                cx.waker().wake_by_ref();
            }
        }
    }
}

impl<V, GlobalSync, NH> PoolManager<V, GlobalSync, NH, ConsensusMode>
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
        ConsensusMode::get_proposable_orders(self)
    }

    /// Handle consensus-specific events
    ///
    /// This can be extended in the future to handle consensus streams,
    /// pre-proposal processing, consensus round transitions, etc.
    pub fn on_consensus_event(&mut self, _event: ()) {
        // Handle consensus-specific events here
        // This could include pre-proposal processing, consensus round
        // transitions, etc.
    }
}
