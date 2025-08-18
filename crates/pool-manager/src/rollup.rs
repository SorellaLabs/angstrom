use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Waker}
};

use angstrom_eth::manager::EthEvent;
use angstrom_types::{block_sync::BlockSyncConsumer, sol_bindings::grouped_orders::AllOrders};
use order_pool::{
    OrderIndexer, PoolConfig, PoolInnerEvent, PoolManagerUpdate, order_storage::OrderStorage
};
use reth_tasks::TaskSpawner;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio_stream::wrappers::UnboundedReceiverStream;
use validation::order::OrderValidatorHandle;

use crate::{
    PoolManager,
    common::PoolManagerCommon,
    impl_common_getters, manager,
    order::{MODULE_NAME, OrderCommand, PoolHandle}
};

/// Rollup mode: no networking state.
#[derive(Debug, Default, Clone, Copy)]
pub struct RollupMode;

// Public type aliases to mirror previous concrete types
pub type RollupPoolManager<V, GS> = PoolManager<V, GS, RollupMode>;

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

        manager::spawn_manager(
            task_spawner,
            self.eth_network_events,
            inner,
            rx,
            self.global_sync,
            RollupMode
        );

        handle
    }
}

impl<V, GS> RollupPoolManager<V, GS>
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

// Implement common getters and shared eth-event handler via PoolManagerCommon
impl<V, GS> PoolManagerCommon<V, GS> for PoolManager<V, GS, RollupMode>
where
    V: OrderValidatorHandle<Order = AllOrders> + Unpin,
    GS: BlockSyncConsumer
{
    impl_common_getters!(PoolManager<V, GS, RollupMode>, V, GS);

    fn on_command(&mut self, cmd: OrderCommand) {
        match cmd {
            OrderCommand::NewOrder(origin, order, validation_response) => {
                self.handle_new_order(origin, order, validation_response)
            }
            OrderCommand::CancelOrder(req, receiver) => {
                let res = self.handle_cancel(req);
                let _ = receiver.send(res);
            }
            OrderCommand::PendingOrders(from, receiver) => {
                let _ = receiver.send(self.handle_pending_orders(from));
            }
            OrderCommand::OrderStatus(order_hash, tx) => {
                let _ = tx.send(self.handle_order_status(order_hash));
            }
            OrderCommand::OrdersByPool(pool_id, location, tx) => {
                let _ = tx.send(self.handle_orders_by_pool(pool_id, location));
            }
        }
    }

    fn on_pool_events(&mut self, orders: Vec<PoolInnerEvent>, waker: impl Fn() -> Waker) {
        for order in orders {
            match order {
                PoolInnerEvent::HasTransitionedToNewBlock(block) => {
                    self.global_sync
                        .sign_off_on_block(MODULE_NAME, block, Some(waker()));
                }
                PoolInnerEvent::None
                | PoolInnerEvent::Propagation(_)
                | PoolInnerEvent::BadOrderMessages(_) => {
                    // No networking / consensus in rollup mode.
                }
            }
        }
    }
}

impl<V, GS> Future for PoolManager<V, GS, RollupMode>
where
    V: OrderValidatorHandle<Order = AllOrders> + Unpin,
    GS: BlockSyncConsumer
{
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        use crate::common::PoolManagerCommon;

        PoolManagerCommon::poll_eth_and_pool(this, cx);
        PoolManagerCommon::drain_commands_if_synced(this, cx);

        Poll::Pending
    }
}
