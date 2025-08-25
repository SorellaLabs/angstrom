//! Generic PoolManager with type-state modes.
//!
//! Mirrors the pattern used by `amm-quoter`: a single generic manager
//! struct parameterized by a mode type that carries mode-specific state
//! and behavior.

use angstrom_eth::manager::EthEvent;
use angstrom_types::{block_sync::BlockSyncConsumer, sol_bindings::grouped_orders::AllOrders};
use futures::Future;
use order_pool::OrderIndexer;
use reth_tasks::TaskSpawner;
use tokio_stream::wrappers::UnboundedReceiverStream;
use validation::order::OrderValidatorHandle;

use crate::handle::OrderCommand;

/// Shared, generic PoolManager state.
pub struct PoolManager<V, GS, M>
where
    V: OrderValidatorHandle<Order = AllOrders> + Unpin,
    GS: BlockSyncConsumer
{
    pub(crate) order_indexer:      OrderIndexer<V>,
    pub(crate) global_sync:        GS,
    pub(crate) eth_network_events: UnboundedReceiverStream<EthEvent>,
    pub(crate) command_rx:         UnboundedReceiverStream<OrderCommand>,
    pub(crate) mode:               M
}

impl<V, GS, M> PoolManager<V, GS, M>
where
    V: OrderValidatorHandle<Order = AllOrders> + Unpin,
    GS: BlockSyncConsumer
{
    pub fn all_orders(&mut self) -> Vec<AllOrders> {
        self.order_indexer
            .get_all_orders_with_parked()
            .into_all_orders()
    }

    pub(crate) fn handle_new_order(
        &mut self,
        origin: angstrom_types::orders::OrderOrigin,
        order: AllOrders,
        validation_response: tokio::sync::oneshot::Sender<
            validation::order::OrderValidationResults
        >
    ) {
        let blocknum = self.global_sync.current_block_number();
        telemetry_recorder::telemetry_event!(blocknum, origin, order.clone());

        self.order_indexer
            .new_rpc_order(origin, order, validation_response);
    }

    pub(crate) fn handle_cancel(
        &mut self,
        req: angstrom_types::orders::CancelOrderRequest
    ) -> bool {
        let blocknum = self.global_sync.current_block_number();
        telemetry_recorder::telemetry_event!(blocknum, req.clone());
        self.order_indexer.cancel_order(&req)
    }

    pub(crate) fn handle_pending_orders(
        &mut self,
        from: alloy::primitives::Address
    ) -> Vec<AllOrders> {
        self.order_indexer
            .pending_orders_for_address(from)
            .into_iter()
            .map(|o| o.order)
            .collect()
    }

    pub(crate) fn handle_order_status(
        &mut self,
        hash: alloy::primitives::B256
    ) -> Option<angstrom_types::orders::OrderStatus> {
        self.order_indexer.order_status(hash)
    }

    pub(crate) fn handle_orders_by_pool(
        &mut self,
        pool_id: alloy::primitives::FixedBytes<32>,
        loc: angstrom_types::orders::OrderLocation
    ) -> Vec<AllOrders> {
        self.order_indexer.orders_by_pool(pool_id, loc)
    }
}

pub(crate) fn spawn_manager<V, GS, M, TP: TaskSpawner>(
    task_spawner: TP,
    eth_network_events: UnboundedReceiverStream<EthEvent>,
    order_indexer: OrderIndexer<V>,
    command_rx: UnboundedReceiverStream<OrderCommand>,
    global_sync: GS,
    mode: M
) where
    V: OrderValidatorHandle<Order = AllOrders> + Unpin + Clone + 'static,
    GS: BlockSyncConsumer + 'static,
    M: 'static,
    PoolManager<V, GS, M>: Future<Output = ()> + Send + 'static
{
    task_spawner.spawn_critical(
        "order pool manager",
        Box::pin(PoolManager { eth_network_events, order_indexer, command_rx, global_sync, mode })
    );
}
