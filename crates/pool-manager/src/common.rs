//! Common traits and functionality shared between pool manager implementations

use angstrom_eth::manager::EthEvent;
use angstrom_types::{
    block_sync::BlockSyncConsumer,
    primitive::{NewInitializedPool, PoolId},
    sol_bindings::grouped_orders::AllOrders
};
use order_pool::{OrderIndexer, PoolInnerEvent};
use tokio_stream::wrappers::UnboundedReceiverStream;
use validation::order::OrderValidatorHandle;

use crate::order::{MODULE_NAME, OrderCommand};

/// Common behavior shared between different pool manager implementations
pub trait PoolManagerCommon {
    type Validator: OrderValidatorHandle<Order = AllOrders> + Unpin;
    type GlobalSync: BlockSyncConsumer;

    /// Get a reference to the order indexer
    fn order_indexer(&self) -> &OrderIndexer<Self::Validator>;

    /// Get a mutable reference to the order indexer
    fn order_indexer_mut(&mut self) -> &mut OrderIndexer<Self::Validator>;

    /// Get a reference to the global sync
    fn global_sync(&self) -> &Self::GlobalSync;

    /// Get a mutable reference to the global sync
    fn global_sync_mut(&mut self) -> &mut Self::GlobalSync;

    /// Get a mutable reference to the eth network events stream
    fn eth_network_events_mut(&mut self) -> &mut UnboundedReceiverStream<EthEvent>;

    /// Get a mutable reference to the command receiver stream
    fn command_rx_mut(&mut self) -> &mut UnboundedReceiverStream<OrderCommand>;

    /// Handle Ethereum events - common implementation
    fn on_eth_event(&mut self, eth: EthEvent, waker: std::task::Waker) {
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
    fn on_pool_events(&mut self, orders: Vec<PoolInnerEvent>, waker: impl Fn() -> std::task::Waker);
}

/// Helper macro to implement common getters for pool managers
#[macro_export]
macro_rules! impl_pool_manager_common_getters {
    ($struct_name:ident < $($gen:ident),* >) => {
        impl<$($gen),*> $crate::common::PoolManagerCommon for $struct_name<$($gen),*>
        where
            V: validation::order::OrderValidatorHandle<Order = angstrom_types::sol_bindings::grouped_orders::AllOrders> + Unpin,
            GS: angstrom_types::block_sync::BlockSyncConsumer
        {
            type Validator = V;
            type GlobalSync = GS;

            fn order_indexer(&self) -> &order_pool::OrderIndexer<Self::Validator> {
                &self.order_indexer
            }

            fn order_indexer_mut(&mut self) -> &mut order_pool::OrderIndexer<Self::Validator> {
                &mut self.order_indexer
            }

            fn global_sync(&self) -> &Self::GlobalSync {
                &self.global_sync
            }

            fn global_sync_mut(&mut self) -> &mut Self::GlobalSync {
                &mut self.global_sync
            }

            fn eth_network_events_mut(&mut self) -> &mut tokio_stream::wrappers::UnboundedReceiverStream<angstrom_eth::manager::EthEvent> {
                &mut self.eth_network_events
            }

            fn command_rx_mut(&mut self) -> &mut tokio_stream::wrappers::UnboundedReceiverStream<$crate::order::OrderCommand> {
                &mut self.command_rx
            }
        }
    };
}
