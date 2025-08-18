//! Common traits and functionality shared between pool manager implementations

use std::task::{Context, Poll, Waker};

use angstrom_eth::manager::EthEvent;
use angstrom_types::{
    block_sync::BlockSyncConsumer,
    primitive::{NewInitializedPool, PoolId},
    sol_bindings::grouped_orders::AllOrders
};
use futures::StreamExt;
use order_pool::{OrderIndexer, PoolInnerEvent};
use tokio_stream::wrappers::UnboundedReceiverStream;
use validation::order::OrderValidatorHandle;

use crate::{MODULE_NAME, handle::OrderCommand};

/// Common behavior shared between different pool manager implementations
pub trait PoolManagerCommon<V, GS>
where
    V: OrderValidatorHandle<Order = AllOrders> + Unpin,
    GS: BlockSyncConsumer
{
    /// Get a reference to the order indexer
    fn order_indexer(&self) -> &OrderIndexer<V>;

    /// Get a mutable reference to the order indexer
    fn order_indexer_mut(&mut self) -> &mut OrderIndexer<V>;

    /// Get a reference to the global sync
    fn global_sync(&self) -> &GS;

    /// Get a mutable reference to the global sync
    fn global_sync_mut(&mut self) -> &mut GS;

    /// Get a mutable reference to the eth network events stream
    fn eth_network_events_mut(&mut self) -> &mut UnboundedReceiverStream<EthEvent>;

    /// Get a mutable reference to the command receiver stream
    fn command_rx_mut(&mut self) -> &mut UnboundedReceiverStream<OrderCommand>;

    /// Handle Ethereum events - common implementation
    fn on_eth_event(&mut self, eth: EthEvent, waker: Waker) {
        match eth {
            EthEvent::NewBlockTransitions { block_number, filled_orders, address_changeset } => {
                self.order_indexer_mut().start_new_block_processing(
                    block_number,
                    filled_orders,
                    address_changeset
                );
                waker.clone().wake_by_ref();
            }
            EthEvent::ReorgedOrders(orders, range) => {
                self.order_indexer_mut().reorg(orders);
                self.global_sync_mut()
                    .sign_off_reorg(MODULE_NAME, range, Some(waker))
            }
            EthEvent::FinalizedBlock(block) => {
                self.order_indexer_mut().finalized_block(block);
            }
            EthEvent::NewPool { pool } => {
                let t0 = pool.currency0;
                let t1 = pool.currency1;
                let id: PoolId = pool.into();

                let pool = NewInitializedPool { currency_in: t0, currency_out: t1, id };

                self.order_indexer_mut().new_pool(pool);
            }
            EthEvent::RemovedPool { pool } => {
                self.order_indexer_mut().remove_pool(pool.into());
            }
            EthEvent::AddedNode(_) => {}
            EthEvent::RemovedNode(_) => {}
            EthEvent::NewBlock(_) => {}
        }
    }

    /// Handle commands - mode-specific implementation required
    fn on_command(&mut self, cmd: OrderCommand);

    /// Handle pool events - mode-specific implementation required
    fn on_pool_events(&mut self, orders: Vec<PoolInnerEvent>, waker: impl Fn() -> Waker);

    /// Default helper: poll eth events and pool indexer
    fn poll_eth_and_pool(&mut self, cx: &mut Context<'_>) {
        while let Poll::Ready(Some(eth)) = self.eth_network_events_mut().poll_next_unpin(cx) {
            self.on_eth_event(eth, cx.waker().clone());
        }

        while let Poll::Ready(Some(orders)) = self.order_indexer_mut().poll_next_unpin(cx) {
            self.on_pool_events(orders, || cx.waker().clone());
        }
    }

    /// Default helper: drain command queue if global sync allows progress
    fn drain_commands_if_synced(&mut self, cx: &mut Context<'_>) {
        if self.global_sync().can_operate() {
            while let Poll::Ready(Some(cmd)) = self.command_rx_mut().poll_next_unpin(cx) {
                self.on_command(cmd);
            }
        }
    }
}

/// Simple macro to implement the repetitive getter methods for
/// PoolManagerCommon
#[macro_export]
macro_rules! impl_common_getters {
    ($struct_name:ty, $validator:ty, $global_sync:ty) => {
        fn order_indexer(&self) -> &OrderIndexer<$validator> {
            &self.order_indexer
        }

        fn order_indexer_mut(&mut self) -> &mut OrderIndexer<$validator> {
            &mut self.order_indexer
        }

        fn global_sync(&self) -> &$global_sync {
            &self.global_sync
        }

        fn global_sync_mut(&mut self) -> &mut $global_sync {
            &mut self.global_sync
        }

        fn eth_network_events_mut(&mut self) -> &mut UnboundedReceiverStream<EthEvent> {
            &mut self.eth_network_events
        }

        fn command_rx_mut(&mut self) -> &mut UnboundedReceiverStream<OrderCommand> {
            &mut self.command_rx
        }
    };
}
