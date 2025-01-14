use std::{
    collections::{HashMap, HashSet},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::{Duration, SystemTime, UNIX_EPOCH}
};

use alloy::primitives::{Address, BlockNumber, FixedBytes, B256, U256};
use angstrom_types::{
    orders::{OrderId, OrderLocation, OrderOrigin, OrderSet, OrderStatus},
    primitive::{NewInitializedPool, PeerId, PoolId},
    sol_bindings::{
        grouped_orders::{AllOrders, OrderWithStorageData, *},
        rpc_orders::TopOfBlockOrder,
        RawPoolOrder
    }
};
use futures_util::{Stream, StreamExt};
use tokio::sync::oneshot::Sender;
use tracing::{error, trace};
use validation::order::{
    state::{account::user::UserAddress, pools::AngstromPoolsTracker},
    OrderValidationResults, OrderValidatorHandle
};

use crate::{
    order_storage::OrderStorage,
    validator::{OrderValidator, OrderValidatorRes},
    PoolManagerUpdate
};

/// This is used to remove validated orders. During validation
/// the same check wil be ran but with more accuracy
const ETH_BLOCK_TIME: Duration = Duration::from_secs(12);
/// mostly arbitrary
const SEEN_INVALID_ORDERS_CAPACITY: usize = 10000;
/// represents the maximum number of blocks that we allow for new orders to not
/// propagate (again mostly arbitrary)
const MAX_NEW_ORDER_DELAY_PROPAGATION: u64 = 7000;

struct CancelOrderRequest {
    /// The address of the entity requesting the cancellation.
    pub from:        Address,
    // The time until the cancellation request is valid.
    pub valid_until: u64
}

pub struct OrderIndexer<V: OrderValidatorHandle> {
    /// order storage
    order_storage:          Arc<OrderStorage>,
    /// Address to order id, used for eoa invalidation
    address_to_orders:      HashMap<Address, Vec<OrderId>>,
    /// current block_number
    block_number:           u64,
    /// Order hash to order id, used for order inclusion lookups
    order_hash_to_order_id: HashMap<B256, OrderId>,
    /// Used to get trigger reputation side-effects on network order submission
    order_hash_to_peer_id:  HashMap<B256, Vec<PeerId>>,
    /// Used to avoid unnecessary computation on order spam
    seen_invalid_orders:    HashSet<B256>,
    /// Used to protect against late order propagation
    cancelled_orders:       HashMap<B256, CancelOrderRequest>,
    /// Order Validator
    validator:              OrderValidator<V>,
    /// a mapping of tokens to pool_id
    pool_id_map:            AngstromPoolsTracker,
    /// List of subscribers for order validation result
    order_validation_subs:  HashMap<B256, Vec<Sender<OrderValidationResults>>>,
    /// List of subscribers for order state change notifications
    orders_subscriber_tx:   tokio::sync::broadcast::Sender<PoolManagerUpdate>
}

impl<V: OrderValidatorHandle<Order = AllOrders>> OrderIndexer<V> {
    pub fn new(
        validator: V,
        order_storage: Arc<OrderStorage>,
        block_number: BlockNumber,
        orders_subscriber_tx: tokio::sync::broadcast::Sender<PoolManagerUpdate>,
        angstrom_pools: AngstromPoolsTracker
    ) -> Self {
        Self {
            order_storage,
            block_number,
            address_to_orders: HashMap::new(),
            order_hash_to_order_id: HashMap::new(),
            order_hash_to_peer_id: HashMap::new(),
            seen_invalid_orders: HashSet::with_capacity(SEEN_INVALID_ORDERS_CAPACITY),
            pool_id_map: angstrom_pools,
            cancelled_orders: HashMap::new(),
            order_validation_subs: HashMap::new(),
            validator: OrderValidator::new(validator),
            orders_subscriber_tx
        }
    }

    pub fn pending_orders_for_address(
        &self,
        address: Address
    ) -> Vec<OrderWithStorageData<AllOrders>> {
        let mut orders = Vec::new();
        if let Some(order_ids) = self.address_to_orders.get(&address) {
            for order_id in order_ids {
                let order = match order_id.location {
                    angstrom_types::orders::OrderLocation::Limit => self
                        .order_storage
                        .limit_orders
                        .lock()
                        .expect("lock poisoned")
                        .get_order(order_id)
                        .and_then(|order| order.try_map_inner(|inner| Ok(inner.into())).ok()),
                    angstrom_types::orders::OrderLocation::Searcher => self
                        .order_storage
                        .searcher_orders
                        .lock()
                        .expect("lock poisoned")
                        .get_order(order_id.pool_id, order_id.hash)
                        .and_then(|order| {
                            order.try_map_inner(|inner| Ok(AllOrders::TOB(inner))).ok()
                        })
                };

                if let Some(order) = order {
                    orders.push(order);
                }
            }
        }
        orders
    }

    pub fn orders_by_pool(
        &self,
        pool_id: FixedBytes<32>,
        order_location: OrderLocation
    ) -> Vec<AllOrders> {
        match order_location {
            OrderLocation::Limit => self
                .order_storage
                .limit_orders
                .lock()
                .expect("poisoned")
                .get_all_orders_from_pool(pool_id),
            OrderLocation::Searcher => self
                .order_storage
                .searcher_orders
                .lock()
                .expect("poisoned")
                .get_all_orders_from_pool(pool_id)
        }
    }

    pub fn order_status(&self, order_hash: B256) -> Option<OrderStatus> {
        self.order_storage.fetch_status_of_order(order_hash)
    }

    fn is_missing(&self, order_hash: &B256) -> bool {
        !self.order_hash_to_order_id.contains_key(order_hash)
    }

    fn is_seen_invalid(&self, order_hash: &B256) -> bool {
        self.seen_invalid_orders.contains(order_hash)
    }

    fn is_cancelled(&self, order_hash: &B256) -> bool {
        self.cancelled_orders.contains_key(order_hash)
    }

    pub fn remove_pool(&self, key: PoolId) {
        self.order_storage.remove_pool(key);
    }

    fn is_duplicate(&self, order_hash: &B256) -> bool {
        if self.order_hash_to_order_id.contains_key(order_hash) || self.is_seen_invalid(order_hash)
        {
            trace!(?order_hash, "got duplicate order");
            return true
        }

        false
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
            return true
        }

        // the cancel arrived before the new order request
        // nothing more needs to be done, since new_order() will return early
        if self.is_missing(&request.order_id) {
            // optimistically assuming that orders won't take longer than a day to propagate
            let deadline = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                + MAX_NEW_ORDER_DELAY_PROPAGATION * ETH_BLOCK_TIME.as_secs();
            self.insert_cancel_request_with_deadline(
                request.user_address,
                &request.order_id,
                Some(U256::from(deadline))
            );

            return true
        }
        let id = self.order_hash_to_order_id.remove(&request.order_id);
        if let Some(order) = id.and_then(|v| self.order_storage.cancel_order(&v)) {
            self.order_hash_to_order_id.remove(&order.order_hash());
            self.order_hash_to_peer_id.remove(&order.order_hash());
            self.insert_cancel_request_with_deadline(
                request.user_address,
                &request.order_id,
                order.deadline()
            );

            self.notify_order_subscribers(PoolManagerUpdate::CancelledOrder {
                order_hash: order.order_hash(),
                user:       order.from(),
                pool_id:    order.pool_id
            });
            return true
        }

        false
    }

    fn insert_cancel_request_with_deadline(
        &mut self,
        from: Address,
        order_hash: &B256,
        deadline: Option<U256>
    ) {
        let valid_until = deadline.map_or_else(
            || {
                // if no deadline is provided the cancellation request is valid until block
                // transition
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
            },
            |deadline| {
                let bytes: [u8; U256::BYTES] = deadline.to_le_bytes();
                // should be safe
                u64::from_le_bytes(bytes[..8].try_into().unwrap())
            }
        );
        self.cancelled_orders
            .insert(*order_hash, CancelOrderRequest { from, valid_until });
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
            self.order_validation_subs
                .entry(hash)
                .or_default()
                .push(validation_tx);
        }

        let cancel_request = self.cancelled_orders.get(&hash);
        let is_valid_cancel_request =
            cancel_request.is_some() && cancel_request.unwrap().from == order.from();
        // network spammers will get penalized only once
        if self.is_duplicate(&hash) || is_valid_cancel_request {
            if is_valid_cancel_request {
                self.insert_cancel_request_with_deadline(order.from(), &hash, order.deadline());

                if let Some(pool_id) = self
                    .pool_id_map
                    .get_poolid(order.token_in(), order.token_out())
                {
                    self.notify_order_subscribers(PoolManagerUpdate::CancelledOrder {
                        order_hash: order.order_hash(),
                        pool_id,
                        user: order.from()
                    });
                }
                self.order_storage.log_cancel_order(&order);
            }
            self.notify_validation_subscribers(&hash, OrderValidationResults::Invalid(hash));
            return
        }

        let hash = order.order_hash();
        if let Some(peer) = peer_id {
            self.order_hash_to_peer_id
                .entry(hash)
                .or_default()
                .push(peer);
        }

        self.validator.validate_order(origin, order);
    }

    /// used to remove orders that expire before the next ethereum block
    fn remove_expired_orders(&mut self, block_number: BlockNumber) -> Vec<B256> {
        self.block_number = block_number;
        let time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        let expiry_deadline = U256::from((time + ETH_BLOCK_TIME).as_secs()); // grab all expired hashes
        let hashes = self
            .order_hash_to_order_id
            .iter()
            .filter(|(_, v)| {
                v.deadline.map(|i| i <= expiry_deadline).unwrap_or_default()
                    || v.flash_block
                        .map(|b| b != block_number + 1)
                        .unwrap_or_default()
            })
            .map(|(k, _)| *k)
            .collect::<Vec<_>>();

        // TODO: notify rpc of dead orders
        let _expired_orders = hashes
            .iter()
            // remove hash from id
            .map(|hash| self.order_hash_to_order_id.remove(hash).unwrap())
            .inspect(|order_id| {
                self.address_to_orders
                    .values_mut()
                    // remove from address to orders
                    .for_each(|v| v.retain(|o| o != order_id));
            })
            // remove from all underlying pools
            .filter_map(|id| match id.location {
                OrderLocation::Searcher => self.order_storage.remove_searcher_order(&id),
                OrderLocation::Limit => self.order_storage.remove_limit_order(&id)
            })
            .collect::<Vec<_>>();

        hashes
    }

    fn eoa_state_change(&mut self, eoas: &[Address]) {
        eoas.iter()
            .filter_map(|eoa| self.address_to_orders.remove(eoa))
            .for_each(|order_ids| {
                order_ids.into_iter().for_each(|id| {
                    let Some(order) = (match id.location {
                        OrderLocation::Limit => self.order_storage.remove_limit_order(&id),
                        OrderLocation::Searcher => self.order_storage.remove_searcher_order(&id)
                    }) else {
                        return
                    };

                    self.validator
                        .validate_order(OrderOrigin::Local, order.order);
                })
            });
    }

    pub fn finalized_block(&mut self, block_number: BlockNumber) {
        self.order_storage.finalized_block(block_number);
    }

    pub fn reorg(&mut self, orders: Vec<B256>) {
        self.order_storage
            .reorg(orders)
            .into_iter()
            .for_each(|order| {
                self.notify_order_subscribers(PoolManagerUpdate::UnfilledOrders(order.clone()));
                self.validator
                    .validate_order(OrderOrigin::Local, order.order)
            });
    }

    /// Removes all filled orders from the pools and moves to regular pool
    fn filled_orders(&mut self, block_number: BlockNumber, orders: &[B256]) {
        if orders.is_empty() {
            return
        }

        let filled_orders = orders
            .iter()
            .filter_map(|hash| self.order_hash_to_order_id.remove(hash))
            .filter_map(|order_id| match order_id.location {
                OrderLocation::Limit => self.order_storage.remove_limit_order(&order_id),
                OrderLocation::Searcher => self.order_storage.remove_searcher_order(&order_id)
            })
            .collect::<Vec<OrderWithStorageData<AllOrders>>>();

        filled_orders.iter().for_each(|order| {
            self.notify_order_subscribers(PoolManagerUpdate::FilledOrder(
                block_number,
                order.clone()
            ));
        });
        self.order_storage
            .add_filled_orders(block_number, filled_orders);
    }

    /// Given the nonce ordering rule. Sometimes new transactions can park old
    /// transactions.
    fn park_transactions(&mut self, txes: &[B256]) {
        let order_info = txes
            .iter()
            .filter_map(|tx_hash| self.order_hash_to_order_id.get(tx_hash))
            .collect::<Vec<_>>();
        self.order_storage.park_orders(order_info);
    }

    fn handle_validated_order(
        &mut self,
        res: OrderValidationResults
    ) -> eyre::Result<PoolInnerEvent> {
        match res {
            OrderValidationResults::Valid(valid) => {
                let hash = valid.order_hash();

                // what about the deadline?
                if valid.valid_block != self.block_number {
                    self.notify_validation_subscribers(
                        &hash,
                        OrderValidationResults::Invalid(hash)
                    );

                    self.seen_invalid_orders.insert(hash);
                    let peers = self.order_hash_to_peer_id.remove(&hash).unwrap_or_default();
                    return Ok(PoolInnerEvent::BadOrderMessages(peers))
                }

                self.notify_order_subscribers(PoolManagerUpdate::NewOrder(valid.clone()));
                self.notify_validation_subscribers(
                    &hash,
                    OrderValidationResults::Valid(valid.clone())
                );

                let to_propagate = valid.order.clone();
                self.update_order_tracking(&hash, valid.from(), valid.order_id);
                self.park_transactions(&valid.invalidates);
                self.insert_order(valid)?;

                Ok(PoolInnerEvent::Propagation(to_propagate))
            }
            OrderValidationResults::Invalid(bad_hash) => {
                self.notify_validation_subscribers(
                    &bad_hash,
                    OrderValidationResults::Invalid(bad_hash)
                );
                self.seen_invalid_orders.insert(bad_hash);
                let peers = self
                    .order_hash_to_peer_id
                    .remove(&bad_hash)
                    .unwrap_or_default();
                Ok(PoolInnerEvent::BadOrderMessages(peers))
            }
            OrderValidationResults::TransitionedToBlock => Ok(PoolInnerEvent::None)
        }
    }

    fn notify_order_subscribers(&mut self, update: PoolManagerUpdate) {
        let _ = self.orders_subscriber_tx.send(update);
    }

    fn notify_validation_subscribers(&mut self, hash: &B256, result: OrderValidationResults) {
        if let Some(subscribers) = self.order_validation_subs.remove(hash) {
            for subscriber in subscribers {
                if let Err(e) = subscriber.send(result.clone()) {
                    error!("Failed to send order validation result to subscriber: {:?}", e);
                }
            }
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

    fn update_order_tracking(&mut self, hash: &B256, user: UserAddress, id: OrderId) {
        self.order_hash_to_peer_id.remove(hash);
        self.order_hash_to_order_id.insert(*hash, id);
        // nonce overlap is checked during validation so its ok we
        // don't check for duplicates
        self.address_to_orders.entry(user).or_default().push(id);
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
        // deal with changed orders
        self.eoa_state_change(&address_changes);
        // deal with filled orders
        self.filled_orders(block_number, &completed_orders);
        // add expired orders to completed
        completed_orders.extend(self.remove_expired_orders(block_number));

        let time_now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        self.cancelled_orders
            .retain(|_, request| request.valid_until >= time_now);

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

        if validated.is_empty() {
            Poll::Pending
        } else {
            Poll::Ready(Some(validated))
        }
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
    use std::sync::Arc;

    use alloy::{primitives::U256, signers::SignerSync, sol_types::SolValue};
    use angstrom_types::{
        contract_bindings::angstrom::Angstrom::PoolKey,
        contract_payloads::angstrom::AngstromPoolConfigStore,
        orders::OrderId,
        primitive::AngstromSigner,
        sol_bindings::{grouped_orders::GroupedVanillaOrder, RespendAvoidanceMethod}
    };
    use revm::primitives::keccak256;
    use testing_tools::{
        mocks::validator::MockValidator, type_generator::orders::UserOrderBuilder
    };
    use tokio::sync::broadcast;
    use tracing_subscriber::{fmt, EnvFilter};

    use super::*;
    use crate::PoolConfig;

    fn setup_test_indexer() -> OrderIndexer<MockValidator> {
        init_tracing();
        let (tx, _) = broadcast::channel(100);
        let order_storage = Arc::new(OrderStorage::new(&PoolConfig::default()));
        let validator = MockValidator::default();
        let pools_tracker =
            AngstromPoolsTracker::new(Address::ZERO, Arc::new(AngstromPoolConfigStore::default()));

        OrderIndexer::new(validator, order_storage, 1, tx, pools_tracker)
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
                is_currently_valid: true,
                is_valid: true,
                priority_data: Default::default(),
                invalidates: vec![],
                tob_reward: U256::ZERO
            }))
            .unwrap();

        // Verify order was added
        assert!(indexer.order_hash_to_order_id.contains_key(&order_hash));

        // Wait for order to expire
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        // Simulate block transition
        let expired_hashes = indexer.remove_expired_orders(2);

        // Verify order was removed
        assert!(expired_hashes.contains(&order_hash));
        assert!(!indexer.order_hash_to_order_id.contains_key(&order_hash));
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
                is_currently_valid: true,
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
        assert!(!indexer.order_hash_to_order_id.contains_key(&order_hash));
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
        assert!(indexer.order_hash_to_peer_id.contains_key(&order_hash));
        assert_eq!(indexer.order_hash_to_peer_id[&order_hash], vec![peer_id]);

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
                is_currently_valid: true,
                is_valid: true,
                priority_data: Default::default(),
                invalidates: vec![],
                tob_reward: U256::ZERO
            }))
            .unwrap();

        // Verify peer tracking is cleared after validation
        assert!(!indexer.order_hash_to_peer_id.contains_key(&order_hash));
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
            .handle_validated_order(OrderValidationResults::Invalid(order_hash))
            .unwrap();

        // Verify order was marked as invalid
        assert!(indexer.seen_invalid_orders.contains(&order_hash));

        // Verify validation result
        match rx.await {
            Ok(OrderValidationResults::Invalid(hash)) => assert_eq!(hash, order_hash),
            _ => panic!("Expected invalid order result")
        }
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
                is_currently_valid: true,
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
                is_currently_valid: true,
                is_valid: true,
                priority_data: Default::default(),
                invalidates: vec![],
                tob_reward: U256::ZERO
            }))
            .unwrap();

        // Verify order was added
        assert!(indexer.order_hash_to_order_id.contains_key(&order_hash));
        assert!(indexer.address_to_orders.contains_key(&from));
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
                is_currently_valid: true,
                is_valid: true,
                priority_data: Default::default(),
                invalidates: vec![],
                tob_reward: U256::ZERO
            }))
            .unwrap();

        let hash = keccak256((from, order_hash).abi_encode());
        let sig = signer.sign_hash_sync(&hash).unwrap();

        // Cancel the order
        let cancel_request = angstrom_types::orders::CancelOrderRequest {
            order_id:     order_hash,
            user_address: from,
            signature:    sig
        };

        let result = indexer.cancel_order(&cancel_request);
        assert!(result);
        assert!(indexer.cancelled_orders.contains_key(&order_hash));
        assert!(!indexer.order_hash_to_order_id.contains_key(&order_hash));
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
                is_currently_valid: true,
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
            Ok(OrderValidationResults::Invalid(hash)) => assert_eq!(hash, order_hash),
            _ => panic!("Expected invalid order result")
        }
    }
}
