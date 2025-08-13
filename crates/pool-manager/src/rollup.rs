use std::sync::Arc;

use angstrom_eth::manager::EthEvent;
use angstrom_types::{block_sync::BlockSyncConsumer, sol_bindings::grouped_orders::AllOrders};
use order_pool::{
    OrderIndexer, PoolConfig, PoolManagerUpdate, order_storage::OrderStorage
};
use reth_tasks::TaskSpawner;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio_stream::wrappers::UnboundedReceiverStream;
use validation::order::OrderValidatorHandle;

use crate::{
    manager::{PoolManager, RollupMode},
    order::{MODULE_NAME, OrderCommand, PoolHandle}
};

/// Builder for constructing RollupPoolManager instances.
pub struct RollupPoolManagerBuilder<V, GlobalSync>
where
    V: OrderValidatorHandle,
    GlobalSync: BlockSyncConsumer
{
    validator:          V,
    global_sync:        GlobalSync,
    order_storage:      Option<Arc<OrderStorage>>,
    eth_network_events: UnboundedReceiverStream<EthEvent>,
    config:             PoolConfig
}

impl<V, GlobalSync> RollupPoolManagerBuilder<V, GlobalSync>
where
    V: OrderValidatorHandle<Order = AllOrders> + Unpin,
    GlobalSync: BlockSyncConsumer
{
    pub fn new(
        validator: V,
        order_storage: Option<Arc<OrderStorage>>,
        eth_network_events: UnboundedReceiverStream<EthEvent>,
        global_sync: GlobalSync
    ) -> Self {
        Self {
            global_sync,
            eth_network_events,
            validator,
            order_storage,
            config: Default::default()
        }
    }

    pub fn with_config(mut self, config: PoolConfig) -> Self {
        self.config = config;
        self
    }

    pub fn with_storage(mut self, order_storage: Arc<OrderStorage>) -> Self {
        let _ = self.order_storage.insert(order_storage);
        self
    }

    pub fn build_with_channels<TP: TaskSpawner>(
        self,
        task_spawner: TP,
        tx: UnboundedSender<OrderCommand>,
        rx: UnboundedReceiver<OrderCommand>,
        pool_manager_tx: tokio::sync::broadcast::Sender<PoolManagerUpdate>,
        block_number: u64,
        replay: impl FnOnce(&mut OrderIndexer<V>) + Send + 'static
    ) -> PoolHandle {
        let rx = UnboundedReceiverStream::new(rx);
        let order_storage = self
            .order_storage
            .unwrap_or_else(|| Arc::new(OrderStorage::new(&self.config)));
        let handle =
            PoolHandle { manager_tx: tx.clone(), pool_manager_tx: pool_manager_tx.clone() };
        let mut inner = OrderIndexer::new(
            self.validator.clone(),
            order_storage.clone(),
            block_number,
            pool_manager_tx.clone()
        );
        replay(&mut inner);
        self.global_sync.register(MODULE_NAME);

        task_spawner.spawn_critical(
            "order pool manager",
            Box::pin(PoolManager {
                eth_network_events: self.eth_network_events,
                order_indexer:      inner,
                command_rx:         rx,
                global_sync:        self.global_sync,
                mode:               RollupMode,
            })
        );

        handle
    }
}

impl<V, GS> super::manager::RollupPoolManager<V, GS>
where
    V: OrderValidatorHandle<Order = AllOrders> + Unpin,
    GS: BlockSyncConsumer
{
    /// Create a new rollup pool manager builder
    pub fn new(
        validator: V,
        order_storage: Option<Arc<OrderStorage>>,
        eth_network_events: UnboundedReceiverStream<EthEvent>,
        global_sync: GS
    ) -> RollupPoolManagerBuilder<V, GS> {
        RollupPoolManagerBuilder::new(validator, order_storage, eth_network_events, global_sync)
    }
}
// All runtime behavior is implemented on the generic manager
