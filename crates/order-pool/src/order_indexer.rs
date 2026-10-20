use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll}
};

use alloy::primitives::{Address, B256, BlockNumber, U256};
use angstrom_types::{
    orders::{OrderId, OrderOrigin, OrderSet},
    primitive::{NewInitializedPool, OrderLocation, OrderStatus, PeerId, PoolId},
    sol_bindings::{
        RawPoolOrder,
        grouped_orders::{AllOrders, OrderWithStorageData},
        rpc_orders::TopOfBlockOrder
    }
};
use futures_util::{Stream, StreamExt};
use tokio::sync::oneshot::Sender;
use validation::order::{OrderValidationResults, OrderValidatorHandle};

use crate::{
    PoolManagerUpdate,
    order_storage::OrderStorage,
    order_subscribers::OrderSubscriptionTracker,
    order_tracker::OrderTracker,
    telemetry::OrderPoolSnapshot,
    validator::{OrderValidator, OrderValidatorRes}
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct InnerCancelOrderRequest {
    /// The address of the entity requesting the cancellation.
    pub from:           Address,
    pub deadline:       U256,
    /// Older snapshots also stored unknown-order claims in this cache. Only
    /// a cancellation bound to a validated owner may authorize propagation.
    #[serde(default)]
    pub owner_verified: bool
}

pub struct OrderIndexer<V: OrderValidatorHandle> {
    /// order storage
    pub(crate) order_storage: Arc<OrderStorage>,
    /// current block_number
    pub(crate) block_number:  u64,
    /// used to track the relevant order ids.
    pub(crate) order_tracker: OrderTracker,
    /// Order hash to order id, used for order inclusion lookups
    /// Order Validator
    pub(crate) validator:     OrderValidator<V>,
    /// List of subscribers for order validation result
    /// order
    pub(crate) subscribers:   OrderSubscriptionTracker
}

impl<V: OrderValidatorHandle<Order = AllOrders>> OrderIndexer<V> {
    pub fn new(
        validator: V,
        order_storage: Arc<OrderStorage>,
        block_number: BlockNumber,
        orders_subscriber_tx: tokio::sync::broadcast::Sender<PoolManagerUpdate>
    ) -> Self {
        Self {
            order_storage,
            order_tracker: OrderTracker::default(),
            block_number,
            validator: OrderValidator::new(validator),
            subscribers: OrderSubscriptionTracker::new(orders_subscriber_tx)
        }
    }

    pub fn set_tracker(&mut self, tracker: OrderTracker) {
        self.order_tracker = tracker;
    }

    pub fn pending_orders_for_address(
        &self,
        address: Address
    ) -> Vec<OrderWithStorageData<AllOrders>> {
        self.order_tracker.pending_orders_for_address(
            address,
            &self.order_storage,
            |order_id, storage| storage.get_order_from_id(order_id)
        )
    }

    pub fn orders_by_pool(&self, pool_id: PoolId, location: OrderLocation) -> Vec<AllOrders> {
        self.order_storage.get_orders_by_pool(pool_id, location)
    }

    pub fn order_status(&self, order_hash: B256) -> Option<OrderStatus> {
        self.order_storage.fetch_status_of_order(order_hash)
    }

    fn is_seen_invalid(&self, order_hash: &B256) -> bool {
        self.order_tracker.is_seen_invalid(order_hash)
    }

    pub fn remove_pool(&mut self, key: PoolId) {
        let orders = self.order_storage.remove_pool(key);
        self.order_tracker.handle_pool_removed(&orders);

        self.subscribers.notify_expired_orders(&orders);
    }

    pub fn new_rpc_order(
        &mut self,
        origin: OrderOrigin,
        order: AllOrders,
        validation_tx: tokio::sync::oneshot::Sender<OrderValidationResults>
    ) {
        self.new_order(None, origin, order, Some(validation_tx))
    }

    pub fn new_network_order(&mut self, peer_id: PeerId, origin: OrderOrigin, order: AllOrders) {
        self.new_order(Some(peer_id), origin, order, None)
    }

    pub fn cancel_order(&mut self, request: &angstrom_types::orders::CancelOrderRequest) -> bool {
        // ensure validity
        if !request.is_valid() {
            return false;
        }

        if self
            .order_tracker
            .order_hash_to_order_id
            .get(&request.order_id)
            .is_some_and(|id| id.address != request.user_address)
        {
            return false;
        }

        // Only an owner-matching, confirmed cancellation is idempotently
        // successful. Unknown-order claims must not be broadcast as authorized.
        if self
            .order_tracker
            .cancelled_orders
            .get(&request.order_id)
            .is_some_and(|request_order| {
                request_order.owner_verified && request_order.from == request.user_address
            })
        {
            return true;
        }

        if self.is_seen_invalid(&request.order_id) {
            return false;
        }

        if let Some((is_tob, pool_id)) = self.order_tracker.cancel_order(
            request.user_address,
            request.order_id,
            &self.order_storage
        ) {
            self.validator
                .cancel_order(request.user_address, request.order_id);
            // grab all parked orders and see if there valid now
            self.order_tracker
                .pending_orders_for_address(
                    request.user_address,
                    &self.order_storage,
                    // we only re validate parked orders.
                    |order_id, storage| storage.remove_parked_order(order_id)
                )
                .into_iter()
                .for_each(|order| {
                    self.validator
                        .validate_order(OrderOrigin::ReValidation, order.order);
                });

            self.subscribers
                .notify_order_subscribers(PoolManagerUpdate::CancelledOrder {
                    is_tob,
                    order_hash: request.order_id,
                    user: request.user_address,
                    pool_id
                });
            return true;
        }

        false
    }

    fn new_order(
        &mut self,
        peer_id: Option<PeerId>,
        origin: OrderOrigin,
        order: AllOrders,
        validation_res_sub: Option<Sender<OrderValidationResults>>
    ) {
        let hash = order.order_hash();
        if let Some(validation_tx) = validation_res_sub {
            self.subscribers.subscribe_to_order(hash, validation_tx);
        }

        // if the order has been canceled, we just notify the validation
        // subscribers that its a cancelled order
        if self.order_tracker.is_valid_cancel(&hash, order.from()) {
            // we only try to notify here as there is a condition where a cancel
            // occurs while we are validating.
            self.subscribers.notify_validation_subscribers(
                &hash,
                OrderValidationResults::Invalid {
                    hash,
                    error: angstrom_types::primitive::OrderValidationError::CancelledOrder
                }
            );
            self.order_storage.log_cancel_order(&order);
            return;
        }

        if self.order_tracker.is_duplicate(&hash) {
            self.subscribers.notify_validation_subscribers(
                &hash,
                OrderValidationResults::Invalid {
                    hash,
                    error: angstrom_types::primitive::OrderValidationError::DuplicateOrder
                }
            );
            return;
        }

        self.order_tracker.track_peer_id(hash, peer_id);

        if self.order_tracker.is_validating(&hash) {
            return;
        }
        self.order_tracker.start_validating(hash);

        self.validator.validate_order(origin, order);
    }

    fn eoa_state_change(&mut self, eoas: &[Address]) {
        self.order_tracker.eoa_state_changes(
            eoas,
            &self.order_storage,
            &mut self.validator,
            |id, storage, validator| {
                if let Some(order) = storage.remove_order_from_id(id) {
                    validator.validate_order(OrderOrigin::ReValidation, order.order);
                }
            }
        );
    }

    pub fn finalized_block(&mut self, block_number: BlockNumber) {
        self.order_storage.finalized_block(block_number);
    }

    pub fn reorg(&mut self, orders: Vec<B256>) {
        self.order_storage
            .reorg(orders)
            .into_iter()
            .for_each(|order| {
                self.subscribers
                    .notify_order_subscribers(PoolManagerUpdate::UnfilledOrders(order.clone()));
                self.validator
                    .validate_order(OrderOrigin::Local, order.order)
            });
    }

    /// Removes all filled orders from the pools and moves to regular pool
    fn filled_orders(&mut self, block_number: BlockNumber, orders: &[B256]) {
        if orders.is_empty() {
            return;
        }

        let filled_orders = self
            .order_tracker
            .filled_orders(orders, &self.order_storage)
            .inspect(|order| {
                self.subscribers
                    .notify_order_subscribers(PoolManagerUpdate::FilledOrder(
                        block_number,
                        order.clone()
                    ));
            })
            .collect();

        self.order_storage
            .add_filled_orders(block_number, filled_orders);
    }

    /// Given the nonce ordering rule. Sometimes new transactions can park old
    /// transactions.
    fn handle_validated_order(
        &mut self,
        res: OrderValidationResults
    ) -> eyre::Result<PoolInnerEvent> {
        match res {
            OrderValidationResults::Valid(valid) => {
                let hash = valid.order_hash();
                self.order_tracker.stop_validating(&hash);

                if valid.valid_block != self.block_number {
                    self.subscribers.try_notify_validation_subscribers(
                        &hash,
                        OrderValidationResults::Invalid {
                            hash,
                            error:
                                angstrom_types::primitive::OrderValidationError::InvalidOrderAtBlock
                        }
                    );

                    let peers = self.order_tracker.invalid_verification(hash);
                    return Ok(PoolInnerEvent::BadOrderMessages(peers));
                }
                // Cancellation can arrive while validation is in flight. Only
                // the validated owner's signed claim can stop insertion and
                // propagation of the result.
                if self.order_tracker.is_valid_cancel(&hash, valid.from()) {
                    self.validator.cancel_order(valid.from(), hash);
                    // Validation already released or flagged the orders this
                    // one displaced (e.g. a same-nonce replacement); revalidate
                    // them so the validator reserves funds for them again.
                    self.revalidate_invalidated_orders(&valid.invalidates);
                    self.order_tracker.order_hash_to_peer_id.remove(&hash);
                    let was_announced = self
                        .order_tracker
                        .order_hash_to_order_id
                        .remove(&hash)
                        .is_some();
                    self.order_tracker.insert_cancel_with_deadline(
                        valid.from(),
                        &hash,
                        valid.deadline()
                    );
                    self.order_storage.log_cancel_order(&valid.order);
                    // A revalidated order was already published as `NewOrder`.
                    if was_announced {
                        self.subscribers.notify_order_subscribers(
                            PoolManagerUpdate::CancelledOrder {
                                is_tob:     matches!(
                                    valid.order_id.location,
                                    OrderLocation::Searcher
                                ),
                                order_hash: hash,
                                user:       valid.from(),
                                pool_id:    valid.pool_id
                            }
                        );
                    }
                    self.subscribers.try_notify_validation_subscribers(
                        &hash,
                        OrderValidationResults::Invalid {
                            hash,
                            error: angstrom_types::primitive::OrderValidationError::CancelledOrder
                        }
                    );
                    return Ok(PoolInnerEvent::None);
                }
                self.subscribers
                    .notify_order_subscribers(PoolManagerUpdate::NewOrder(valid.clone()));

                // check to see if the transaction is parked.
                if let Some(ref error) = valid.is_currently_valid {
                    self.subscribers.try_notify_validation_subscribers(
                        &hash,
                        OrderValidationResults::Invalid {
                            hash,
                            error: angstrom_types::primitive::OrderValidationError::StateError(
                                error.clone()
                            )
                        }
                    );
                } else {
                    self.subscribers.try_notify_validation_subscribers(
                        &hash,
                        OrderValidationResults::Valid(valid.clone())
                    );
                }

                let to_propagate = valid.order.clone();
                self.order_tracker
                    .new_valid_order(&hash, valid.from(), valid.order_id);

                self.revalidate_invalidated_orders(&valid.invalidates);

                if let Err(e) = self.insert_order(valid) {
                    tracing::error!(%e, "failed to insert valid order");
                }

                Ok(PoolInnerEvent::Propagation(to_propagate))
            }
            this @ OrderValidationResults::Invalid { hash, .. } => {
                self.order_tracker.stop_validating(&hash);
                self.subscribers
                    .try_notify_validation_subscribers(&hash, this);
                self.order_storage.remove_invalid_order(hash);
                let peers = self.order_tracker.invalid_verification(hash);
                Ok(PoolInnerEvent::BadOrderMessages(peers))
            }
            OrderValidationResults::TransitionedToBlock(new_gas_updates) => {
                let parked_orders = self
                    .order_storage
                    .apply_new_gas_and_return_blocked_orders(new_gas_updates);

                for order in parked_orders {
                    self.validator
                        .validate_order(OrderOrigin::ReValidation, order);
                }
                Ok(PoolInnerEvent::None)
            }
        }
    }

    /// For all the orders that get invalided, we will re-validate to calculate
    /// the proper amount of funding to unblock them.
    fn revalidate_invalidated_orders(&mut self, invalidates: &[B256]) {
        let invalided_orders = invalidates
            .iter()
            .flat_map(|order| {
                let id = self.order_tracker.order_hash_to_order_id.get(order)?;
                self.order_storage.remove_order_from_id(id)
            })
            .collect::<Vec<_>>();

        for order in invalided_orders {
            self.order_tracker.start_validating(order.order_hash());
            self.validator
                .validate_order(OrderOrigin::ReValidation, order.order);
        }
    }

    fn insert_order(&mut self, res: OrderWithStorageData<AllOrders>) -> eyre::Result<()> {
        match res.order_id.location {
            angstrom_types::primitive::OrderLocation::Searcher => self
                .order_storage
                .add_new_searcher_order(
                    res.try_map_inner(|inner| {
                        let AllOrders::TOB(order) = inner else { eyre::bail!("unreachable") };
                        Ok(order)
                    })
                    .expect("should be unreachable")
                )
                .map_err(|e| eyre::anyhow!("{:?}", e)),
            angstrom_types::primitive::OrderLocation::Limit => self
                .order_storage
                .add_new_limit_order(res)
                .map_err(|e| eyre::anyhow!("{:?}", e))
        }
    }

    /// This should only be used when building the Proposal. This is because
    /// we want to ignore cancelled orders as if the cancellation happened after
    /// consensus closed. we ignore these.
    pub fn get_all_orders_with_cancelled(&self) -> OrderSet<AllOrders, TopOfBlockOrder> {
        self.order_storage
            .get_all_orders_with_ingoing_cancellations()
    }

    pub fn get_all_orders_with_parked(&self) -> OrderSet<AllOrders, TopOfBlockOrder> {
        self.order_storage.get_all_orders_with_parked()
    }

    pub fn get_all_orders(&self) -> OrderSet<AllOrders, TopOfBlockOrder> {
        self.order_storage.get_all_orders()
    }

    pub fn new_pool(&self, pool: NewInitializedPool) {
        self.order_storage.new_pool(pool);
    }

    pub fn start_new_block_processing(
        &mut self,
        block_number: BlockNumber,
        completed_orders: Vec<B256>,
        address_changes: Vec<Address>
    ) {
        tracing::info!(%block_number, "starting transition to new block processing");
        self.validator
            .on_new_block(block_number, completed_orders, address_changes);
    }

    // given that we cant acutally remove orders on cancel.
    pub fn purge_cancelled(&mut self) {
        for order in self.order_storage.purge_tob_orders() {
            self.order_tracker.insert_cancel_with_deadline(
                order.from(),
                &order.order_hash(),
                order.deadline()
            );
        }

        for order in self.order_storage.purge_user_orders() {
            self.order_tracker.insert_cancel_with_deadline(
                order.from(),
                &order.order_hash(),
                order.deadline()
            );
        }
    }

    fn finish_new_block_processing(
        &mut self,
        block_number: BlockNumber,
        mut completed_orders: Vec<B256>,
        address_changes: Vec<Address>
    ) {
        self.block_number = block_number;
        telemetry_recorder::telemetry_event!(OrderPoolSnapshot::from((block_number, &*self)));
        // clear the invalid orders as they could of become valid.
        self.order_tracker.clear_invalid();
        // deal with filled orders
        self.filled_orders(block_number, &completed_orders);

        self.purge_cancelled();
        let expired_orders = self
            .order_tracker
            .remove_expired_orders(block_number, &self.order_storage);
        self.subscribers.notify_expired_orders(&expired_orders);

        // deal with changed orders
        self.eoa_state_change(&address_changes);

        // Given we retain cancelled tobs given we want to look them up
        // if the cancel occured after consensus, we want to ensure
        // we properly clear them out now that they are no longer needed.

        completed_orders.extend(expired_orders.into_iter().map(|o| o.order_id.hash));

        self.validator.notify_validation_on_changes(
            block_number,
            completed_orders,
            address_changes
        );
    }
}

impl<V> Stream for OrderIndexer<V>
where
    V: OrderValidatorHandle<Order = AllOrders>
{
    type Item = Vec<PoolInnerEvent>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut validated = Vec::new();

        while let Poll::Ready(Some(next)) = self.validator.poll_next_unpin(cx) {
            match next {
                OrderValidatorRes::EnsureClearForTransition { block, orders, addresses } => {
                    tracing::info!(
                        "ensure clear for transition. pruning all old + invalid txes from the pool"
                    );
                    self.finish_new_block_processing(block, orders, addresses);
                }
                OrderValidatorRes::ValidatedOrder(next) => {
                    if let Ok(prop) = self.handle_validated_order(next) {
                        validated.push(prop);
                    }
                }
                OrderValidatorRes::TransitionComplete => {
                    validated.push(PoolInnerEvent::HasTransitionedToNewBlock(self.block_number));
                }
            }
        }

        if validated.is_empty() { Poll::Pending } else { Poll::Ready(Some(validated)) }
    }
}

#[allow(clippy::large_enum_variant)]
pub enum PoolInnerEvent {
    Propagation(AllOrders),
    BadOrderMessages(Vec<PeerId>),
    HasTransitionedToNewBlock(u64),
    None
}

#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
pub enum PoolError {
    #[error("Pool has reached max size, and order doesn't satisify replacment requirements")]
    MaxSize,
    #[error("No pool was found for address: {0}")]
    NoPool(PoolId),
    #[error("Already have a ordered with {0:?}")]
    DuplicateNonce(OrderId),
    #[error("Duplicate order")]
    DuplicateOrder
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashSet,
        sync::Arc,
        time::{SystemTime, UNIX_EPOCH}
    };

    use alloy::{primitives::U256, signers::local::PrivateKeySigner};
    use angstrom_types::{
        contract_bindings::angstrom::Angstrom::PoolKey,
        matching::Ray,
        orders::{CancelOrderRequest, OrderId, builders::ToBOrderBuilder},
        primitive::{AngstromAddressConfig, AngstromSigner, OrderValidationError},
        sol_bindings::RespendAvoidanceMethod
    };
    use testing_tools::{
        mocks::validator::MockValidator, type_generator::orders::UserOrderBuilder
    };
    use tokio::sync::broadcast;
    use tracing_subscriber::{EnvFilter, fmt};

    use super::*;
    use crate::PoolConfig;

    fn setup_test_indexer() -> OrderIndexer<MockValidator> {
        init_tracing();
        AngstromAddressConfig::INTERNAL_TESTNET.try_init();
        let (tx, _) = broadcast::channel(100);
        let order_storage = Arc::new(OrderStorage::new(&PoolConfig::default()));
        let validator = MockValidator::default();

        OrderIndexer::new(validator, order_storage, 1, tx)
    }

    fn setup_test_indexer_with_fn(
        mut f: impl FnMut(&mut MockValidator)
    ) -> OrderIndexer<MockValidator> {
        init_tracing();
        let (tx, _) = broadcast::channel(100);
        let order_storage = Arc::new(OrderStorage::new(&PoolConfig::default()));
        let mut validator = MockValidator::default();
        f(&mut validator);

        OrderIndexer::new(validator, order_storage, 1, tx)
    }

    /// Initialize the tracing subscriber for tests
    fn init_tracing() {
        let _ = fmt()
            .with_env_filter(
                EnvFilter::from_default_env()
                    .add_directive("order_pool=debug".parse().unwrap())
                    .add_directive("info".parse().unwrap())
            )
            .with_test_writer()
            .try_init();
    }

    #[derive(Default)]
    struct OrderValidity {
        valid_until: Option<U256>,
        flash_block: Option<u64>,
        is_standing: bool
    }

    fn create_test_order(
        from: Address,
        pool_id: PoolKey,
        validity: Option<OrderValidity>,
        signer: Option<AngstromSigner<PrivateKeySigner>>
    ) -> AllOrders {
        let validity = validity.unwrap_or_default();

        let mut builder = UserOrderBuilder::new()
            .asset_in(pool_id.currency0)
            .asset_out(pool_id.currency1)
            .amount(900)
            .min_price(Ray::from(U256::from(1)))
            .signing_key(signer)
            .recipient(from);

        if let Some(valid_until) = validity.valid_until {
            builder = builder.deadline(valid_until);
            builder = builder.is_standing(true);
        }

        if let Some(flash_block) = validity.flash_block {
            builder = builder.block(flash_block);
        }

        if validity.is_standing { builder.standing() } else { builder.kill_or_fill() }.build()
    }

    #[tokio::test]
    async fn test_expired_orders_handling() {
        let mut indexer = setup_test_indexer();
        let from = Address::random();
        let pool_key = PoolKey {
            currency0: Address::random(),
            currency1: Address::random(),
            ..Default::default()
        };
        let pool_id = PoolId::from(pool_key);

        // Create an order that expires in the next block
        let validity = OrderValidity {
            valid_until: Some(U256::from(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
                    + 1
            )),
            flash_block: None,
            is_standing: true
        };
        indexer.new_pool(NewInitializedPool {
            currency_out: pool_key.currency0,
            currency_in:  pool_key.currency1,
            id:           pool_id
        });
        let order = create_test_order(from, pool_key, Some(validity), None);

        // Submit and validate the order
        let (tx, _) = tokio::sync::oneshot::channel();
        indexer.new_rpc_order(OrderOrigin::Local, order.clone(), tx);

        let order_hash = order.order_hash();
        indexer
            .handle_validated_order(OrderValidationResults::Valid(OrderWithStorageData {
                order: order.clone(),
                order_id: OrderId {
                    address: from,
                    reuse_avoidance: RespendAvoidanceMethod::Nonce(1),
                    hash: order_hash,
                    pool_id,
                    location: OrderLocation::Limit,
                    deadline: Some(U256::from(
                        SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap()
                            .as_secs()
                            + 1
                    )),
                    flash_block: None
                },
                valid_block: 1,
                pool_id,
                cancel_requested: false,
                is_bid: true,
                is_currently_valid: None,
                is_valid: true,
                priority_data: Default::default(),
                invalidates: vec![],
                tob_reward: U256::ZERO
            }))
            .unwrap();

        // Verify order was added
        assert!(
            indexer
                .order_tracker
                .order_hash_to_order_id
                .contains_key(&order_hash)
        );

        // Wait for order to expire
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        // Simulate block transition
        let expired_hashes = indexer
            .order_tracker
            .remove_expired_orders(2, &indexer.order_storage)
            .into_iter()
            .map(|o| o.order_id.hash)
            .collect::<Vec<_>>();

        // Verify order was removed
        assert!(expired_hashes.contains(&order_hash));
        assert!(
            !indexer
                .order_tracker
                .order_hash_to_order_id
                .contains_key(&order_hash)
        );
    }

    #[tokio::test]
    async fn test_block_transitions() {
        let mut indexer = setup_test_indexer();
        let from = Address::random();
        let pool_key = PoolKey {
            currency0: Address::random(),
            currency1: Address::random(),
            ..Default::default()
        };
        let pool_id = PoolId::from(pool_key);
        let validity = OrderValidity {
            valid_until: Some(U256::from(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
                    + 3600 // Valid for 1 hour
            )),
            flash_block: None,
            is_standing: true
        };
        indexer.new_pool(NewInitializedPool {
            currency_out: pool_key.currency0,
            currency_in:  pool_key.currency1,
            id:           pool_id
        });

        let order = create_test_order(from, pool_key, Some(validity), None);
        let order_hash = order.order_hash();

        // Submit and validate order
        let (tx, _) = tokio::sync::oneshot::channel();
        indexer.new_rpc_order(OrderOrigin::Local, order.clone(), tx);

        indexer
            .handle_validated_order(OrderValidationResults::Valid(OrderWithStorageData {
                order: order.clone(),

                cancel_requested: false,

                order_id: OrderId {
                    address: from,
                    reuse_avoidance: RespendAvoidanceMethod::Nonce(1),
                    hash: order_hash,
                    pool_id,
                    location: OrderLocation::Limit,
                    deadline: None,
                    flash_block: None
                },
                valid_block: 1,
                pool_id,
                is_bid: true,
                is_currently_valid: None,
                is_valid: true,
                priority_data: Default::default(),
                invalidates: vec![],
                tob_reward: U256::ZERO
            }))
            .unwrap();

        // Simulate block transition with completed orders
        let completed_orders = vec![order_hash];
        let address_changes = vec![from];

        indexer.finish_new_block_processing(2, completed_orders.clone(), address_changes.clone());

        // Verify order was removed
        assert!(
            !indexer
                .order_tracker
                .order_hash_to_order_id
                .contains_key(&order_hash)
        );
    }

    #[tokio::test]
    async fn test_network_order_handling() {
        let mut indexer = setup_test_indexer();
        let from = Address::random();
        let pool_key = PoolKey {
            currency0: Address::random(),
            currency1: Address::random(),
            ..Default::default()
        };
        let pool_id = PoolId::from(pool_key);

        let validity = OrderValidity {
            valid_until: Some(U256::from(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
                    + 3600
            )),
            flash_block: None,
            is_standing: true
        };
        let order = create_test_order(from, pool_key, Some(validity), None);
        indexer.new_pool(NewInitializedPool {
            currency_out: pool_key.currency0,
            currency_in:  pool_key.currency1,
            id:           pool_id
        });

        let peer_id = PeerId::random();

        // Submit network order
        indexer.new_network_order(peer_id, OrderOrigin::External, order.clone());
        let order_hash = order.order_hash();

        // Verify peer tracking
        assert!(
            indexer
                .order_tracker
                .order_hash_to_peer_id
                .contains_key(&order_hash)
        );
        assert_eq!(indexer.order_tracker.order_hash_to_peer_id[&order_hash], vec![peer_id]);

        // Validate order
        indexer
            .handle_validated_order(OrderValidationResults::Valid(OrderWithStorageData {
                cancel_requested: false,
                order: order.clone(),
                order_id: OrderId {
                    hash: order_hash,
                    address: from,
                    reuse_avoidance: RespendAvoidanceMethod::Nonce(1),
                    pool_id,
                    location: OrderLocation::Limit,
                    deadline: None,
                    flash_block: None
                },
                valid_block: 1,
                pool_id,
                is_bid: true,
                is_currently_valid: None,
                is_valid: true,
                priority_data: Default::default(),
                invalidates: vec![],
                tob_reward: U256::ZERO
            }))
            .unwrap();

        // Verify peer tracking is cleared after validation
        assert!(
            !indexer
                .order_tracker
                .order_hash_to_peer_id
                .contains_key(&order_hash)
        );
    }

    #[tokio::test]
    async fn test_invalid_orders() {
        let mut indexer = setup_test_indexer();
        let from = Address::random();
        let pool_key = PoolKey {
            currency0: Address::random(),
            currency1: Address::random(),
            ..Default::default()
        };
        let order = create_test_order(from, pool_key, None, None);
        let order_hash = order.order_hash();
        indexer.new_pool(NewInitializedPool {
            currency_out: pool_key.currency0,
            currency_in:  pool_key.currency1,
            id:           PoolId::from(pool_key)
        });

        // Submit order and mark as invalid
        let (tx, rx) = tokio::sync::oneshot::channel();
        indexer.new_rpc_order(OrderOrigin::Local, order.clone(), tx);

        indexer
            .handle_validated_order(OrderValidationResults::Invalid {
                hash:  order_hash,
                error: angstrom_types::primitive::OrderValidationError::InvalidPool
            })
            .unwrap();

        // Verify order was marked as invalid
        assert!(indexer.is_seen_invalid(&order_hash));

        // Verify validation result
        match rx.await {
            Ok(OrderValidationResults::Invalid { hash, .. }) => assert_eq!(hash, order_hash),
            _ => panic!("Expected invalid order result")
        }
    }

    #[tokio::test]
    async fn test_invalid_orders_clear() {
        let mut indexer = setup_test_indexer();
        let from = Address::random();
        let pool_key = PoolKey {
            currency0: Address::random(),
            currency1: Address::random(),
            ..Default::default()
        };
        let order = create_test_order(from, pool_key, None, None);
        let order_hash = order.order_hash();
        indexer.new_pool(NewInitializedPool {
            currency_out: pool_key.currency0,
            currency_in:  pool_key.currency1,
            id:           PoolId::from(pool_key)
        });

        // Submit order and mark as invalid
        let (tx, rx) = tokio::sync::oneshot::channel();
        indexer.new_rpc_order(OrderOrigin::Local, order.clone(), tx);

        indexer
            .handle_validated_order(OrderValidationResults::Invalid {
                hash:  order_hash,
                error: angstrom_types::primitive::OrderValidationError::InvalidPool
            })
            .unwrap();

        // Verify order was marked as invalid
        assert!(indexer.is_seen_invalid(&order_hash));

        // Verify validation result
        match rx.await {
            Ok(OrderValidationResults::Invalid { hash, .. }) => assert_eq!(hash, order_hash),
            _ => panic!("Expected invalid order result")
        }

        indexer.finish_new_block_processing(1, vec![], vec![]);
        assert!(!indexer.is_seen_invalid(&order_hash));
    }

    #[tokio::test]
    async fn test_pool_management() {
        let mut indexer = setup_test_indexer();

        let pool_key = PoolKey {
            currency0: Address::random(),
            currency1: Address::random(),
            ..Default::default()
        };
        let pool_id = PoolId::from(pool_key);

        // Create a new pool
        let new_pool = NewInitializedPool {
            id:           pool_id,
            currency_in:  pool_key.currency0,
            currency_out: pool_key.currency1
        };

        indexer.new_pool(new_pool);

        // Add order to pool
        let from = Address::random();
        let order = create_test_order(from, pool_key, None, None);
        let (tx, _) = tokio::sync::oneshot::channel();
        indexer.new_rpc_order(OrderOrigin::Local, order.clone(), tx);

        // Validate order
        let order_hash = order.order_hash();
        indexer
            .handle_validated_order(OrderValidationResults::Valid(OrderWithStorageData {
                cancel_requested: false,
                order: order.clone(),
                order_id: OrderId {
                    address: from,
                    reuse_avoidance: RespendAvoidanceMethod::Nonce(1),
                    hash: order_hash,
                    pool_id,
                    location: OrderLocation::Limit,
                    deadline: None,
                    flash_block: None
                },
                valid_block: 1,
                pool_id,
                is_bid: true,
                is_currently_valid: None,
                is_valid: true,
                priority_data: Default::default(),
                invalidates: vec![],
                tob_reward: U256::ZERO
            }))
            .unwrap();

        // Verify order is in pool
        let pool_orders = indexer.orders_by_pool(pool_id, OrderLocation::Limit);
        assert!(!pool_orders.is_empty());

        // Remove pool
        indexer.remove_pool(pool_id);

        // Verify orders were removed
        let pool_orders = indexer.orders_by_pool(pool_id, OrderLocation::Limit);
        assert!(pool_orders.is_empty());
    }

    #[tokio::test]
    async fn test_new_order_basic() {
        let mut indexer = setup_test_indexer();
        let s = AngstromSigner::random();
        let from = s.address();

        let pool_key = PoolKey {
            currency0: Address::random(),
            currency1: Address::random(),
            ..Default::default()
        };
        let pool_id = PoolId::from(pool_key);
        indexer.new_pool(NewInitializedPool {
            currency_out: pool_key.currency0,
            currency_in:  pool_key.currency1,
            id:           PoolId::from(pool_key)
        });
        let validity = OrderValidity {
            valid_until: Some(U256::from(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
                    + 3600
            )),
            flash_block: None,
            is_standing: true
        };
        let order = create_test_order(from, pool_key, Some(validity), Some(s));
        let order_hash = order.order_hash();

        // Create a channel for validation results
        let (tx, _) = tokio::sync::oneshot::channel();

        // Submit the order
        indexer.new_rpc_order(OrderOrigin::Local, order.clone(), tx);

        // Simulate validation completion
        indexer
            .handle_validated_order(OrderValidationResults::Valid(OrderWithStorageData {
                cancel_requested: false,
                order: order.clone(),
                order_id: OrderId {
                    address: from,
                    reuse_avoidance: RespendAvoidanceMethod::Nonce(1),
                    hash: order_hash,
                    pool_id,
                    location: OrderLocation::Limit,
                    deadline: None,
                    flash_block: None
                },
                valid_block: 1,
                pool_id,
                is_bid: true,
                is_currently_valid: None,
                is_valid: true,
                priority_data: Default::default(),
                invalidates: vec![],
                tob_reward: U256::ZERO
            }))
            .unwrap();

        // Verify order was added
        assert!(
            indexer
                .order_tracker
                .order_hash_to_order_id
                .contains_key(&order_hash)
        );
        assert!(indexer.order_tracker.address_to_orders.contains_key(&from));
    }

    #[tokio::test]
    async fn test_cancel_order() {
        let mut indexer = setup_test_indexer();

        let pool_key = PoolKey {
            currency0: Address::random(),
            currency1: Address::random(),
            ..Default::default()
        };
        let pool_id = PoolId::from(pool_key);
        indexer.new_pool(NewInitializedPool {
            currency_out: pool_key.currency0,
            currency_in:  pool_key.currency1,
            id:           PoolId::from(pool_key)
        });
        let signer = AngstromSigner::random();
        let from = signer.address();

        let order = create_test_order(from, pool_key, None, Some(signer.clone()));
        let order_hash = order.order_hash();

        // Submit and validate the order first
        let (tx, _) = tokio::sync::oneshot::channel();
        indexer.new_rpc_order(OrderOrigin::Local, order.clone(), tx);

        indexer
            .handle_validated_order(OrderValidationResults::Valid(OrderWithStorageData {
                order: order.clone(),
                cancel_requested: false,
                order_id: OrderId {
                    address: from,
                    reuse_avoidance: RespendAvoidanceMethod::Nonce(1),
                    hash: order_hash,
                    pool_id,
                    location: OrderLocation::Limit,
                    deadline: None,
                    flash_block: None
                },
                valid_block: 1,
                pool_id,
                is_bid: true,
                is_currently_valid: None,
                is_valid: true,
                priority_data: Default::default(),
                invalidates: vec![],
                tob_reward: U256::ZERO
            }))
            .unwrap();

        let cancel_request =
            angstrom_types::orders::CancelOrderRequest::new(from, order_hash, &signer);

        let result = indexer.cancel_order(&cancel_request);
        assert!(result);

        assert!(
            indexer
                .order_tracker
                .cancelled_orders
                .contains_key(&order_hash)
        );

        assert!(
            !indexer
                .order_tracker
                .order_hash_to_order_id
                .contains_key(&order_hash)
        );
    }

    fn cancellation_fixture(
        is_tob: bool
    ) -> (
        OrderIndexer<MockValidator>,
        AngstromSigner<PrivateKeySigner>,
        OrderWithStorageData<AllOrders>
    ) {
        let indexer = setup_test_indexer();
        let signer = AngstromSigner::random();
        let pool_key = PoolKey {
            currency0: Address::random(),
            currency1: Address::random(),
            ..Default::default()
        };
        let pool_id = PoolId::from(pool_key);
        indexer.new_pool(NewInitializedPool {
            currency_out: pool_key.currency0,
            currency_in:  pool_key.currency1,
            id:           pool_id
        });
        let order = if is_tob {
            AllOrders::TOB(
                ToBOrderBuilder::new()
                    .asset_in(pool_key.currency0)
                    .asset_out(pool_key.currency1)
                    .quantity_in(900)
                    .quantity_out(800)
                    .valid_block(2)
                    .signing_key(Some(signer.clone()))
                    .build()
            )
        } else {
            create_test_order(signer.address(), pool_key, None, Some(signer.clone()))
        };
        (indexer, signer, validated_order(order, pool_id, vec![]))
    }

    fn validated_order(
        order: AllOrders,
        pool_id: PoolId,
        invalidates: Vec<B256>
    ) -> OrderWithStorageData<AllOrders> {
        OrderWithStorageData {
            order_id: OrderId::from_all_orders(&order, pool_id),
            order,
            cancel_requested: false,
            valid_block: 1,
            pool_id,
            is_bid: true,
            is_currently_valid: None,
            is_valid: true,
            priority_data: Default::default(),
            invalidates,
            tob_reward: U256::ZERO
        }
    }

    /// A replacement of `displaced` by the same owner (a distinct hash).
    fn replacement_order(
        owner: &AngstromSigner<PrivateKeySigner>,
        displaced: &OrderWithStorageData<AllOrders>
    ) -> AllOrders {
        UserOrderBuilder::new()
            .standing()
            .asset_in(displaced.token_in())
            .asset_out(displaced.token_out())
            .amount(900)
            .signing_key(Some(owner.clone()))
            .recipient(Address::random())
            .build()
    }

    #[tokio::test]
    async fn cancel_requires_the_stored_order_owner() {
        for is_tob in [false, true] {
            let (mut indexer, owner, stored) = cancellation_fixture(is_tob);
            let hash = stored.order_hash();
            indexer
                .handle_validated_order(OrderValidationResults::Valid(stored))
                .unwrap();
            let (updates, mut received) = broadcast::channel(10);
            indexer.subscribers = OrderSubscriptionTracker::new(updates);

            let attacker = AngstromSigner::random();
            let attack = CancelOrderRequest::new(attacker.address(), hash, &attacker);
            assert!(attack.is_valid(), "the attacker's own signature is authentic");
            assert!(!indexer.cancel_order(&attack));
            assert!(
                indexer
                    .order_tracker
                    .order_hash_to_order_id
                    .contains_key(&hash)
            );
            assert!(
                !indexer
                    .order_tracker
                    .is_valid_cancel(&hash, owner.address())
            );
            assert!(indexer.order_tracker.pending_cancellations.is_empty());
            assert_eq!(indexer.pending_orders_for_address(owner.address()).len(), 1);
            assert!(matches!(received.try_recv(), Err(broadcast::error::TryRecvError::Empty)));

            // The internal mutation boundary independently enforces ownership.
            assert!(
                indexer
                    .order_tracker
                    .cancel_order(attacker.address(), hash, &indexer.order_storage)
                    .is_none()
            );
            assert_eq!(indexer.pending_orders_for_address(owner.address()).len(), 1);

            let forged = CancelOrderRequest::new(owner.address(), hash, &attacker);
            assert!(!forged.is_valid());
            assert!(!indexer.cancel_order(&forged));
            assert_eq!(indexer.pending_orders_for_address(owner.address()).len(), 1);

            let legitimate = CancelOrderRequest::new(owner.address(), hash, &owner);
            assert!(indexer.cancel_order(&legitimate));
            assert!(matches!(received.try_recv(), Ok(PoolManagerUpdate::CancelledOrder {
                order_hash, user, ..
            }) if order_hash == hash && user == owner.address()));
            let saved = serde_json::to_value(&indexer.order_tracker).unwrap();
            indexer.order_tracker = serde_json::from_value(saved).unwrap();
            assert!(indexer.cancel_order(&legitimate), "owner cancellation is idempotent");
            assert!(!indexer.cancel_order(&attack), "another signer cannot replay cancellation");
            assert!(matches!(received.try_recv(), Err(broadcast::error::TryRecvError::Empty)));
            assert!(
                !indexer
                    .order_tracker
                    .order_hash_to_order_id
                    .contains_key(&hash)
            );
        }
    }

    #[tokio::test]
    async fn cancel_unknown_order_cannot_poison_another_owner() {
        let (mut indexer, owner, stored) = cancellation_fixture(true);
        let hash = stored.order_hash();
        let attacker = AngstromSigner::random();
        let attack = CancelOrderRequest::new(attacker.address(), hash, &attacker);
        assert!(!indexer.cancel_order(&attack));
        assert!(!indexer.cancel_order(&attack), "unverified claims must not trigger gossip");

        let (tx, rx) = tokio::sync::oneshot::channel();
        indexer.new_rpc_order(OrderOrigin::Local, stored.order.clone(), tx);
        assert!(indexer.order_tracker.is_validating(&hash));
        let result = indexer
            .handle_validated_order(OrderValidationResults::Valid(stored))
            .unwrap();
        assert!(matches!(result, PoolInnerEvent::Propagation(_)));
        assert!(matches!(rx.await.unwrap(), OrderValidationResults::Valid(_)));
        assert!(indexer.order_tracker.pending_cancellations.is_empty());
        assert_eq!(indexer.pending_orders_for_address(owner.address()).len(), 1);
        assert!(indexer.cancel_order(&CancelOrderRequest::new(owner.address(), hash, &owner)));
    }

    #[tokio::test]
    async fn cancel_owner_claim_survives_other_signers_before_order_arrival() {
        let (mut indexer, owner, stored) = cancellation_fixture(true);
        let hash = stored.order_hash();
        let attacker = AngstromSigner::random();
        let attack = CancelOrderRequest::new(attacker.address(), hash, &attacker);
        let legitimate = CancelOrderRequest::new(owner.address(), hash, &owner);
        assert!(!indexer.cancel_order(&attack));
        assert!(!indexer.cancel_order(&legitimate));
        assert!(!indexer.cancel_order(&attack));

        // Snapshot round-tripping retains both independent signed claims.
        let encoded = serde_json::to_value(&indexer.order_tracker).unwrap();
        indexer.order_tracker = serde_json::from_value(encoded).unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel();
        indexer.new_rpc_order(OrderOrigin::Local, stored.order, tx);
        assert!(matches!(
            rx.await.unwrap(),
            OrderValidationResults::Invalid { error: OrderValidationError::CancelledOrder, .. }
        ));
        assert!(!indexer.order_tracker.is_validating(&hash));
        assert!(
            !indexer
                .order_tracker
                .order_hash_to_order_id
                .contains_key(&hash)
        );
    }

    #[tokio::test]
    async fn cancel_during_validation_does_not_insert_or_propagate_order() {
        let (mut indexer, owner, stored) = cancellation_fixture(true);
        let hash = stored.order_hash();
        let (updates, mut received) = broadcast::channel(10);
        indexer.subscribers = OrderSubscriptionTracker::new(updates);
        let (tx, rx) = tokio::sync::oneshot::channel();
        indexer.new_rpc_order(OrderOrigin::Local, stored.order.clone(), tx);
        indexer.new_network_order(PeerId::random(), OrderOrigin::External, stored.order.clone());
        assert!(indexer.order_tracker.is_validating(&hash));
        assert!(
            indexer
                .order_tracker
                .order_hash_to_peer_id
                .contains_key(&hash)
        );
        assert!(!indexer.cancel_order(&CancelOrderRequest::new(owner.address(), hash, &owner)));

        let result = indexer
            .handle_validated_order(OrderValidationResults::Valid(stored))
            .unwrap();
        assert!(matches!(result, PoolInnerEvent::None));
        assert!(matches!(
            rx.await.unwrap(),
            OrderValidationResults::Invalid { error: OrderValidationError::CancelledOrder, .. }
        ));
        assert!(matches!(received.try_recv(), Err(broadcast::error::TryRecvError::Empty)));
        assert!(!indexer.order_tracker.is_validating(&hash));
        assert!(
            !indexer
                .order_tracker
                .order_hash_to_peer_id
                .contains_key(&hash)
        );
        assert!(
            !indexer
                .order_tracker
                .order_hash_to_order_id
                .contains_key(&hash)
        );
        assert!(
            indexer
                .pending_orders_for_address(owner.address())
                .is_empty()
        );
        assert!(
            indexer
                .order_tracker
                .is_valid_cancel(&hash, owner.address())
        );
    }

    #[tokio::test]
    async fn cancel_during_revalidation_does_not_restore_order() {
        let (mut indexer, owner, stored) = cancellation_fixture(false);
        let hash = stored.order_hash();
        indexer
            .handle_validated_order(OrderValidationResults::Valid(stored.clone()))
            .unwrap();
        let (updates, mut received) = broadcast::channel(10);
        indexer.subscribers = OrderSubscriptionTracker::new(updates);
        // Revalidation removes storage while retaining the validated owner index.
        assert!(
            indexer
                .order_storage
                .remove_order_from_id(&stored.order_id)
                .is_some()
        );
        indexer.order_tracker.start_validating(hash);
        assert!(!indexer.cancel_order(&CancelOrderRequest::new(owner.address(), hash, &owner)));
        let result = indexer
            .handle_validated_order(OrderValidationResults::Valid(stored))
            .unwrap();
        assert!(matches!(result, PoolInnerEvent::None));
        // Subscribers saw the order before revalidation, so they must see it go.
        assert!(matches!(received.try_recv(), Ok(PoolManagerUpdate::CancelledOrder {
            order_hash, user, ..
        }) if order_hash == hash && user == owner.address()));
        assert!(!indexer.order_tracker.is_validating(&hash));
        assert!(
            !indexer
                .order_tracker
                .order_hash_to_order_id
                .contains_key(&hash)
        );
        assert!(
            indexer
                .pending_orders_for_address(owner.address())
                .is_empty()
        );
    }

    #[tokio::test]
    async fn cancel_during_validation_revalidates_displaced_orders() {
        let (mut indexer, owner, displaced) = cancellation_fixture(false);
        let displaced_hash = displaced.order_hash();
        indexer
            .handle_validated_order(OrderValidationResults::Valid(displaced.clone()))
            .unwrap();
        let replacement = replacement_order(&owner, &displaced);
        let hash = replacement.order_hash();
        let (tx, _rx) = tokio::sync::oneshot::channel();
        indexer.new_rpc_order(OrderOrigin::Local, replacement.clone(), tx);
        assert!(!indexer.cancel_order(&CancelOrderRequest::new(owner.address(), hash, &owner)));

        // Validating the replacement released the displaced order's
        // reservation; cancelling the replacement must not leave the displaced
        // order live in storage without one.
        let result = indexer
            .handle_validated_order(OrderValidationResults::Valid(validated_order(
                replacement,
                displaced.pool_id,
                vec![displaced_hash]
            )))
            .unwrap();
        assert!(matches!(result, PoolInnerEvent::None));
        assert!(
            indexer
                .order_storage
                .get_order_from_id(&displaced.order_id)
                .is_none()
        );
        assert!(indexer.order_tracker.is_validating(&displaced_hash));
    }

    #[test]
    fn cancel_cache_reads_existing_snapshot_shape() {
        let owner = Address::random();
        let hash = B256::random();
        let mut tracker = OrderTracker::default();
        tracker.insert_cancel_with_deadline(owner, &hash, Some(U256::from(1234)));
        let mut legacy = serde_json::to_value(tracker).unwrap();
        legacy
            .as_object_mut()
            .unwrap()
            .remove("pending_cancellations");
        for request in legacy["cancelled_orders"]
            .as_object_mut()
            .unwrap()
            .values_mut()
        {
            request.as_object_mut().unwrap().remove("owner_verified");
        }
        let restored: OrderTracker = serde_json::from_value(legacy).unwrap();
        assert!(restored.is_valid_cancel(&hash, owner));
        assert!(!restored.is_valid_cancel(&hash, Address::random()));
        assert!(!restored.cancelled_orders[&hash].owner_verified);
        assert!(restored.pending_cancellations.is_empty());
    }

    #[tokio::test]
    async fn cancel_legacy_unknown_claim_cannot_authorize_gossip_or_block_victim() {
        let (mut indexer, owner, stored) = cancellation_fixture(true);
        let hash = stored.order_hash();
        let attacker = AngstromSigner::random();
        let mut legacy_tracker = OrderTracker::default();
        legacy_tracker.insert_cancel_with_deadline(attacker.address(), &hash, stored.deadline());
        let mut legacy = serde_json::to_value(legacy_tracker).unwrap();
        legacy
            .as_object_mut()
            .unwrap()
            .remove("pending_cancellations");
        for request in legacy["cancelled_orders"]
            .as_object_mut()
            .unwrap()
            .values_mut()
        {
            request.as_object_mut().unwrap().remove("owner_verified");
        }
        indexer.set_tracker(serde_json::from_value(legacy).unwrap());
        let request = CancelOrderRequest::new(attacker.address(), hash, &attacker);
        assert!(!indexer.cancel_order(&request));
        assert!(!indexer.cancel_order(&request));

        let (tx, rx) = tokio::sync::oneshot::channel();
        indexer.new_rpc_order(OrderOrigin::Local, stored.order.clone(), tx);
        let result = indexer
            .handle_validated_order(OrderValidationResults::Valid(stored))
            .unwrap();
        assert!(matches!(result, PoolInnerEvent::Propagation(_)));
        assert!(matches!(rx.await.unwrap(), OrderValidationResults::Valid(_)));
        assert_eq!(indexer.pending_orders_for_address(owner.address()).len(), 1);
        assert!(!indexer.cancel_order(&request));
        assert!(indexer.cancel_order(&CancelOrderRequest::new(owner.address(), hash, &owner)));
    }

    #[tokio::test]
    async fn cancel_invalid_hash_is_not_authorized_for_gossip() {
        let mut indexer = setup_test_indexer();
        let signer = AngstromSigner::random();
        let hash = B256::random();
        indexer.order_tracker.seen_invalid_orders.insert(hash);
        assert!(!indexer.cancel_order(&CancelOrderRequest::new(signer.address(), hash, &signer)));
        assert!(indexer.order_tracker.pending_cancellations.is_empty());
    }

    #[tokio::test]
    async fn test_duplicate_order_rejection() {
        let from = Address::random();
        let pool_key = PoolKey {
            currency0: Address::random(),
            currency1: Address::random(),
            ..Default::default()
        };
        let pool_id = PoolId::from(pool_key);
        let order = create_test_order(from, pool_key, None, None);
        let order_hash = order.order_hash();

        let mut indexer = setup_test_indexer_with_fn(|validator| {
            validator.add_order(
                order.from(),
                OrderValidationResults::Invalid {
                    hash:  order_hash,
                    error: OrderValidationError::DuplicateOrder
                }
            )
        });

        indexer.new_pool(NewInitializedPool {
            currency_out: pool_key.currency0,
            currency_in:  pool_key.currency1,
            id:           PoolId::from(pool_key)
        });

        // Submit the order first time
        let (tx1, _) = tokio::sync::oneshot::channel();
        indexer.new_rpc_order(OrderOrigin::Local, order.clone(), tx1);

        // Validate first order
        indexer
            .handle_validated_order(OrderValidationResults::Valid(OrderWithStorageData {
                order: order.clone(),
                cancel_requested: false,
                order_id: OrderId {
                    address: from,
                    reuse_avoidance: RespendAvoidanceMethod::Nonce(1),
                    hash: order_hash,
                    pool_id,
                    location: OrderLocation::Limit,
                    deadline: None,
                    flash_block: None
                },
                valid_block: 1,
                pool_id,
                is_bid: true,
                is_currently_valid: None,
                is_valid: true,
                priority_data: Default::default(),
                invalidates: vec![],
                tob_reward: U256::ZERO
            }))
            .unwrap();

        // Try to submit the same order again
        let (tx2, rx2) = tokio::sync::oneshot::channel();
        indexer.new_rpc_order(OrderOrigin::Local, order.clone(), tx2);

        // The duplicate order should be rejected
        match rx2.await {
            Ok(OrderValidationResults::Invalid { hash, .. }) => assert_eq!(hash, order_hash),
            _ => panic!("Expected invalid order result")
        }

        let _ = indexer.next().await;

        // get_all_orders
        let all_order_storage_hashes = indexer
            .order_storage
            .get_all_orders()
            .into_all_orders()
            .into_iter()
            .map(|o| o.order_hash())
            .collect::<HashSet<_>>();
        assert!(!all_order_storage_hashes.contains(&order_hash));

        // assert!(!indexer.order_tracker.or)
    }
}
