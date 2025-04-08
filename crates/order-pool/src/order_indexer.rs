use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll}
};

use alloy::primitives::{Address, B256, BlockNumber, U256};
use angstrom_types::{
    orders::{OrderId, OrderLocation, OrderOrigin, OrderSet, OrderStatus},
    primitive::{NewInitializedPool, PeerId, PoolId},
    sol_bindings::{
        RawPoolOrder,
        grouped_orders::{AllOrders, OrderWithStorageData, *},
        rpc_orders::TopOfBlockOrder
    }
};
use futures_util::{Stream, StreamExt};
use tokio::sync::oneshot::Sender;
use tracing::error;
use validation::order::{OrderValidationResults, OrderValidatorHandle};

use crate::{
    PoolManagerUpdate,
    order_storage::OrderStorage,
    order_subscribers::OrderSubscriptionTracker,
    order_tracker::OrderTracker,
    validator::{OrderValidator, OrderValidatorRes}
};

pub(crate) struct InnerCancelOrderRequest {
    /// The address of the entity requesting the cancellation.
    pub from:     Address,
    pub deadline: U256
}

pub struct OrderIndexer<V: OrderValidatorHandle> {
    /// order storage
    order_storage: Arc<OrderStorage>,
    /// current block_number
    block_number:  u64,
    /// used to track the relevant order ids.
    order_tracker: OrderTracker,
    /// Order hash to order id, used for order inclusion lookups
    /// Order Validator
    validator:     OrderValidator<V>,
    /// List of subscribers for order validation result
    /// order
    subscribers:   OrderSubscriptionTracker
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

    fn is_cancelled(&self, order_hash: &B256) -> bool {
        self.order_tracker.is_cancelled(order_hash)
    }

    pub fn remove_pool(&self, key: PoolId) {
        self.order_storage.remove_pool(key);
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

        if self.is_seen_invalid(&request.order_id) || self.is_cancelled(&request.order_id) {
            return true;
        }

        self.validator
            .cancel_order(request.user_address, request.order_id);

        if let Some(pool_id) = self.order_tracker.cancel_order(
            request.user_address,
            request.order_id,
            &self.order_storage
        ) {
            // grab all parked orders from this address and check if they are now valid.
            self.order_tracker
                .pending_orders_for_address(
                    request.user_address,
                    &self.order_storage,
                    |order_id, storage| storage.remove_order(order_id)
                )
                .into_iter()
                .for_each(|order| {
                    self.validator
                        .validate_order(OrderOrigin::Local, order.order);
                });

            self.subscribers
                .notify_order_subscribers(PoolManagerUpdate::CancelledOrder {
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

        // if the order has been canceled, we just notify the validation subscribers
        // that its a cancelled order
        if self.order_tracker.is_valid_cancel(&hash, order.from()) {
            self.subscribers.notify_validation_subscribers(
                &hash,
                OrderValidationResults::Invalid {
                    hash,
                    error: angstrom_types::primitive::OrderValidationError::CancelledOrder
                }
            );
            self.order_storage.log_cancel_order(&order);
        }

        if self.order_tracker.is_duplicate(&hash) {
            self.subscribers.notify_validation_subscribers(
                &hash,
                OrderValidationResults::Invalid {
                    hash,
                    error: angstrom_types::primitive::OrderValidationError::DuplicateOrder
                }
            );
        }

        // network spammers will get penalized only once
        self.order_tracker.track_peer_id(hash, peer_id);
        self.validator.validate_order(origin, order);
    }

    fn eoa_state_change(&mut self, eoas: &[Address]) {
        self.order_tracker.eoa_state_changes(
            eoas,
            &self.order_storage,
            &mut self.validator,
            |id, storage, validator| {
                if let Some(order) = storage.get_order_from_id(id) {
                    validator.validate_order(OrderOrigin::Local, order.order);
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

                if valid.valid_block != self.block_number {
                    self.subscribers.notify_validation_subscribers(
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
                self.subscribers
                    .notify_order_subscribers(PoolManagerUpdate::NewOrder(valid.clone()));

                // check to see if the transaction is parked.
                if let Some(ref error) = valid.is_currently_valid {
                    self.subscribers.notify_validation_subscribers(
                        &hash,
                        OrderValidationResults::Invalid {
                            hash,
                            error: angstrom_types::primitive::OrderValidationError::StateError(
                                error.clone()
                            )
                        }
                    );
                } else {
                    self.subscribers.notify_validation_subscribers(
                        &hash,
                        OrderValidationResults::Valid(valid.clone())
                    );
                }

                let to_propagate = valid.order.clone();
                self.order_tracker
                    .new_valid_order(&hash, valid.from(), valid.order_id);
                self.order_tracker
                    .park_orders(&valid.invalidates, &self.order_storage);

                if let Err(e) = self.insert_order(valid) {
                    tracing::error!(%e, "failed to insert valid order");
                }

                Ok(PoolInnerEvent::Propagation(to_propagate))
            }
            this @ OrderValidationResults::Invalid { hash, .. } => {
                self.subscribers.notify_validation_subscribers(&hash, this);
                let peers = self.order_tracker.invalid_verification(hash);
                Ok(PoolInnerEvent::BadOrderMessages(peers))
            }
            OrderValidationResults::TransitionedToBlock => Ok(PoolInnerEvent::None)
        }
    }

    fn insert_order(&mut self, res: OrderWithStorageData<AllOrders>) -> eyre::Result<()> {
        match res.order_id.location {
            angstrom_types::orders::OrderLocation::Searcher => self
                .order_storage
                .add_new_searcher_order(
                    res.try_map_inner(|inner| {
                        let AllOrders::TOB(order) = inner else { eyre::bail!("unreachable") };
                        Ok(order)
                    })
                    .expect("should be unreachable")
                )
                .map_err(|e| eyre::anyhow!("{:?}", e)),
            angstrom_types::orders::OrderLocation::Limit => self
                .order_storage
                .add_new_limit_order(
                    res.try_map_inner(|inner| {
                        Ok(match inner {
                            AllOrders::Standing(p) => {
                                GroupedUserOrder::Vanilla(GroupedVanillaOrder::Standing(p))
                            }
                            AllOrders::Flash(kof) => {
                                GroupedUserOrder::Vanilla(GroupedVanillaOrder::KillOrFill(kof))
                            }
                            _ => eyre::bail!("unreachable")
                        })
                    })
                    .expect("should be unreachable")
                )
                .map_err(|e| eyre::anyhow!("{:?}", e))
        }
    }

    pub fn get_all_orders(&self) -> OrderSet<GroupedVanillaOrder, TopOfBlockOrder> {
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

    fn finish_new_block_processing(
        &mut self,
        block_number: BlockNumber,
        mut completed_orders: Vec<B256>,
        address_changes: Vec<Address>
    ) {
        self.block_number = block_number;
        // clear the invalid orders as they could of become valid.
        self.order_tracker.clear_invalid();
        // deal with changed orders
        self.eoa_state_change(&address_changes);
        // deal with filled orders
        self.filled_orders(block_number, &completed_orders);
        // add expired orders to completed
        completed_orders.extend(
            self.order_tracker
                .remove_expired_orders(block_number, &self.order_storage)
        );

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
        sync::Arc,
        time::{SystemTime, UNIX_EPOCH}
    };

    use alloy::{primitives::U256, signers::SignerSync, sol_types::SolValue};
    use angstrom_types::{
        contract_bindings::angstrom::Angstrom::PoolKey,
        matching::Ray,
        orders::OrderId,
        primitive::AngstromSigner,
        sol_bindings::{RespendAvoidanceMethod, grouped_orders::GroupedVanillaOrder}
    };
    use pade::PadeEncode;
    use testing_tools::{
        mocks::validator::MockValidator, type_generator::orders::UserOrderBuilder
    };
    use tokio::sync::broadcast;
    use tracing_subscriber::{EnvFilter, fmt};

    use super::*;
    use crate::PoolConfig;

    fn setup_test_indexer() -> OrderIndexer<MockValidator> {
        init_tracing();
        let (tx, _) = broadcast::channel(100);
        let order_storage = Arc::new(OrderStorage::new(&PoolConfig::default()));
        let validator = MockValidator::default();

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
        signer: Option<AngstromSigner>
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

        let order =
            if validity.is_standing { builder.standing() } else { builder.kill_or_fill() }.build();

        match order {
            GroupedVanillaOrder::Standing(o) => AllOrders::Standing(o),
            GroupedVanillaOrder::KillOrFill(o) => AllOrders::Flash(o)
        }
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
        let pool_id = PoolId::from(pool_key.clone());

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
            .remove_expired_orders(2, &indexer.order_storage);

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
        let pool_id = PoolId::from(pool_key.clone());
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

        let order = create_test_order(from, pool_key.clone(), Some(validity), None);
        let order_hash = order.order_hash();

        // Submit and validate order
        let (tx, _) = tokio::sync::oneshot::channel();
        indexer.new_rpc_order(OrderOrigin::Local, order.clone(), tx);

        indexer
            .handle_validated_order(OrderValidationResults::Valid(OrderWithStorageData {
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
        let pool_id = PoolId::from(pool_key.clone());

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
        let order = create_test_order(from, pool_key.clone(), Some(validity), None);
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
        let order = create_test_order(from, pool_key.clone(), None, None);
        let order_hash = order.order_hash();
        indexer.new_pool(NewInitializedPool {
            currency_out: pool_key.currency0,
            currency_in:  pool_key.currency1,
            id:           PoolId::from(pool_key.clone())
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
        let order = create_test_order(from, pool_key.clone(), None, None);
        let order_hash = order.order_hash();
        indexer.new_pool(NewInitializedPool {
            currency_out: pool_key.currency0,
            currency_in:  pool_key.currency1,
            id:           PoolId::from(pool_key.clone())
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
        let pool_id = PoolId::from(pool_key.clone());

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
        let pool_id = PoolId::from(pool_key.clone());
        indexer.new_pool(NewInitializedPool {
            currency_out: pool_key.currency0,
            currency_in:  pool_key.currency1,
            id:           PoolId::from(pool_key.clone())
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
        let pool_id = PoolId::from(pool_key.clone());
        indexer.new_pool(NewInitializedPool {
            currency_out: pool_key.currency0,
            currency_in:  pool_key.currency1,
            id:           PoolId::from(pool_key.clone())
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

        let hash = (from, order_hash).abi_encode_packed();
        let sig = signer.sign_message_sync(hash.as_slice()).unwrap();

        // Cancel the order
        let cancel_request = angstrom_types::orders::CancelOrderRequest {
            order_id:     order_hash,
            user_address: from,
            signature:    sig.pade_encode().into()
        };

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

    #[tokio::test]
    async fn test_duplicate_order_rejection() {
        let mut indexer = setup_test_indexer();
        let from = Address::random();

        let pool_key = PoolKey {
            currency0: Address::random(),
            currency1: Address::random(),
            ..Default::default()
        };
        let pool_id = PoolId::from(pool_key.clone());
        indexer.new_pool(NewInitializedPool {
            currency_out: pool_key.currency0,
            currency_in:  pool_key.currency1,
            id:           PoolId::from(pool_key.clone())
        });
        let order = create_test_order(from, pool_key, None, None);
        let order_hash = order.order_hash();

        // Submit the order first time
        let (tx1, _) = tokio::sync::oneshot::channel();
        indexer.new_rpc_order(OrderOrigin::Local, order.clone(), tx1);

        // Validate first order
        indexer
            .handle_validated_order(OrderValidationResults::Valid(OrderWithStorageData {
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

        // Try to submit the same order again
        let (tx2, rx2) = tokio::sync::oneshot::channel();
        indexer.new_rpc_order(OrderOrigin::Local, order.clone(), tx2);

        // The duplicate order should be rejected
        match rx2.await {
            Ok(OrderValidationResults::Invalid { hash, .. }) => assert_eq!(hash, order_hash),
            _ => panic!("Expected invalid order result")
        }
    }
}
