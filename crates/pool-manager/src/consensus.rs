use std::collections::HashMap;
use std::sync::Arc;

use angstrom_eth::manager::EthEvent;
use angstrom_network::{NetworkHandle, NetworkOrderEvent, StromNetworkEvent};
use angstrom_types::{block_sync::BlockSyncConsumer, sol_bindings::grouped_orders::AllOrders};
use order_pool::{OrderIndexer, order_storage::OrderStorage};
use reth_metrics::common::mpsc::UnboundedMeteredReceiver;
use tokio_stream::wrappers::UnboundedReceiverStream;
use validation::order::OrderValidatorHandle;

use crate::{
    manager::{ConsensusMode, PoolManager},
    order::{MODULE_NAME, OrderCommand, PoolHandle},
};

/// Builder for constructing ConsensusPoolManager instances.
pub struct ConsensusPoolManagerBuilder<V, GlobalSync, NH>
where
    V: OrderValidatorHandle,
    GlobalSync: BlockSyncConsumer,
    NH: NetworkHandle,
{
    validator: V,
    global_sync: GlobalSync,
    order_storage: Option<Arc<OrderStorage>>,
    network_handle: NH,
    eth_network_events: UnboundedReceiverStream<EthEvent>,
    order_events: UnboundedMeteredReceiver<NetworkOrderEvent>,
    strom_network_events: UnboundedReceiverStream<StromNetworkEvent>,
    config: order_pool::PoolConfig,
}

impl<V, GlobalSync, NH> ConsensusPoolManagerBuilder<V, GlobalSync, NH>
where
    V: OrderValidatorHandle<Order = AllOrders> + Unpin,
    GlobalSync: BlockSyncConsumer,
    NH: NetworkHandle<Events<'static> = UnboundedReceiverStream<StromNetworkEvent>>
        + Send
        + Sync
        + Unpin
        + 'static,
{
    pub fn new(
        validator: V,
        order_storage: Option<Arc<OrderStorage>>,
        network_handle: NH,
        eth_network_events: UnboundedReceiverStream<EthEvent>,
        order_events: UnboundedMeteredReceiver<NetworkOrderEvent>,
        global_sync: GlobalSync,
        strom_network_events: UnboundedReceiverStream<StromNetworkEvent>,
    ) -> Self {
        Self {
            validator,
            global_sync,
            order_storage,
            network_handle,
            eth_network_events,
            order_events,
            strom_network_events,
            config: Default::default(),
        }
    }

    pub fn with_config(mut self, config: order_pool::PoolConfig) -> Self {
        self.config = config;
        self
    }

    pub fn with_storage(mut self, order_storage: Arc<OrderStorage>) -> Self {
        let _ = self.order_storage.insert(order_storage);
        self
    }

    pub fn build_with_channels<TP: reth_tasks::TaskSpawner>(
        self,
        task_spawner: TP,
        tx: tokio::sync::mpsc::UnboundedSender<crate::order::OrderCommand>,
        rx: tokio::sync::mpsc::UnboundedReceiver<crate::order::OrderCommand>,
        pool_manager_tx: tokio::sync::broadcast::Sender<order_pool::PoolManagerUpdate>,
        block_number: u64,
        replay: impl FnOnce(&mut order_pool::OrderIndexer<V>) + Send + 'static,
    ) -> crate::order::PoolHandle {
        let rx = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);
        let order_storage = self
            .order_storage
            .unwrap_or_else(|| Arc::new(OrderStorage::new(&self.config)));
        let handle =
            PoolHandle { manager_tx: tx.clone(), pool_manager_tx: pool_manager_tx.clone() };
        let mut inner = order_pool::OrderIndexer::new(
            self.validator.clone(),
            order_storage.clone(),
            block_number,
            pool_manager_tx.clone(),
        );
        replay(&mut inner);
        self.global_sync.register(MODULE_NAME);

        task_spawner.spawn_critical(
            "order pool manager",
            Box::pin(PoolManager {
                eth_network_events: self.eth_network_events,
                order_indexer: inner,
                command_rx: rx,
                global_sync: self.global_sync,
                mode: ConsensusMode {
                    network: self.network_handle,
                    strom_network_events: self.strom_network_events,
                    order_events: self.order_events,
                    peer_to_info: HashMap::new(),
                },
            }),
        );

        handle
    }
}

impl<V, GS, NH> super::manager::ConsensusPoolManager<V, GS, NH>
where
    V: OrderValidatorHandle<Order = AllOrders> + Unpin,
    GS: BlockSyncConsumer,
    NH: NetworkHandle<Events<'static> = UnboundedReceiverStream<StromNetworkEvent>>
        + Send
        + Sync
        + Unpin
        + 'static,
{
    /// Create a new consensus pool manager builder
    pub fn new(
        validator: V,
        order_storage: Option<Arc<OrderStorage>>,
        network_handle: NH,
        eth_network_events: UnboundedReceiverStream<EthEvent>,
        order_events: UnboundedMeteredReceiver<NetworkOrderEvent>,
        global_sync: GS,
        strom_network_events: UnboundedReceiverStream<StromNetworkEvent>,
    ) -> ConsensusPoolManagerBuilder<V, GS, NH> {
        ConsensusPoolManagerBuilder::new(
            validator,
            order_storage,
            network_handle,
            eth_network_events,
            order_events,
            global_sync,
            strom_network_events,
        )
    }
}
