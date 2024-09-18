use std::{
    collections::{HashMap, HashSet},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::{Duration, SystemTime, UNIX_EPOCH}
};

use alloy_primitives::{B256, U256};
use angstrom_types::{
    orders::{OrderId, OrderOrigin, OrderSet},
    primitive::PoolId,
    sol_bindings::{
        grouped_orders::{AllOrders, OrderWithStorageData, *},
        rpc_orders::TopOfBlockOrder
    }
};
use futures_util::{Stream, StreamExt};
use reth_network_peers::PeerId;
use reth_primitives::Address;
use tokio::sync::oneshot::Sender;
use tracing::{error, trace};
use validation::order::{
    state::account::user::UserAddress, OrderValidationResults, OrderValidatorHandle
};

use crate::{
    order_storage::OrderStorage,
    validator::{OrderValidator, OrderValidatorRes},
    PoolManagerUpdate
};

/// This is used to remove validated orders. During validation
/// the same check wil be ran but with more accuracy
const ETH_BLOCK_TIME: Duration = Duration::from_secs(12);

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
    /// Order Validator
    validator:              OrderValidator<V>,
    /// List of subscribers for order validation result
    order_validation_subs:  HashMap<B256, Vec<Sender<OrderValidationResults>>>,
    /// List of subscribers for order state change notifications
    orders_subscriber_tx:   tokio::sync::broadcast::Sender<PoolManagerUpdate>
}

impl<V: OrderValidatorHandle<Order = AllOrders>> OrderIndexer<V> {
    pub fn new(
        validator: V,
        order_storage: Arc<OrderStorage>,
        block_number: u64,
        orders_subscriber_tx: tokio::sync::broadcast::Sender<PoolManagerUpdate>
    ) -> Self {
        Self {
            order_storage,
            block_number,
            address_to_orders: HashMap::new(),
            order_hash_to_order_id: HashMap::new(),
            order_hash_to_peer_id: HashMap::new(),
            seen_invalid_orders: HashSet::with_capacity(10000),
            order_validation_subs: HashMap::new(),
            validator: OrderValidator::new(validator),
            orders_subscriber_tx
        }
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

    fn new_order(
        &mut self,
        peer_id: Option<PeerId>,
        origin: OrderOrigin,
        order: AllOrders,
        validation_res_sub: Option<Sender<OrderValidationResults>>
    ) {
        let hash = order.order_hash();
        // network spammers will get penalized only once
        if self.is_duplicate(&hash) {
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

        if let Some(validation_tx) = validation_res_sub {
            self.order_validation_subs
                .entry(hash)
                .or_default()
                .push(validation_tx);
        }
        self.validator.validate_order(origin, order);
    }

    fn is_duplicate(&self, hash: &B256) -> bool {
        if self.order_hash_to_order_id.contains_key(hash)
            || self.seen_invalid_orders.contains(hash)
        {
            trace!(?hash, "got duplicate order");
            return true
        }

        false
    }

    /// used to remove orders that expire before the next ethereum block
    fn remove_expired_orders(&mut self, block_number: u64) -> Vec<B256> {
        self.block_number = block_number;
        let time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        let expiry_deadline = U256::from((time + ETH_BLOCK_TIME).as_secs()); // grab all expired hashes
        let hashes = self
            .order_hash_to_order_id
            .iter()
            .filter(|(_, v)| {
                v.deadline.map(|i| i <= expiry_deadline).unwrap_or_default()
                    || v.flash_block.map(|b| b != block_number).unwrap_or_default()
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
                angstrom_types::orders::OrderLocation::Searcher => {
                    self.order_storage.remove_searcher_order(&id)
                }
                angstrom_types::orders::OrderLocation::Limit => {
                    self.order_storage.remove_limit_order(&id)
                }
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
                        angstrom_types::orders::OrderLocation::Limit => {
                            self.order_storage.remove_limit_order(&id)
                        }
                        angstrom_types::orders::OrderLocation::Searcher => {
                            self.order_storage.remove_searcher_order(&id)
                        }
                    }) else {
                        return
                    };

                    self.validator
                        .validate_order(OrderOrigin::Local, order.order);
                })
            });
    }

    pub fn finalized_block(&mut self, block: u64) {
        self.order_storage.finalized_block(block);
    }

    pub fn reorg(&mut self, orders: Vec<B256>) {
        self.order_storage
            .reorg(orders)
            .into_iter()
            .for_each(|order| {
                self.notify_order_subscribers(PoolManagerUpdate::UnfilledOrders(order.clone()));
                self.validator.validate_order(OrderOrigin::Local, order)
            });
    }

    /// Removes all filled orders from the pools and moves to regular pool
    fn filled_orders(&mut self, block: u64, orders: &[B256]) {
        if orders.is_empty() {
            return
        }

        let filled_orders = orders
            .iter()
            .filter_map(|hash| self.order_hash_to_order_id.remove(hash))
            .filter_map(|order_id| match order_id.location {
                angstrom_types::orders::OrderLocation::Limit => {
                    self.order_storage.remove_limit_order(&order_id)
                }
                angstrom_types::orders::OrderLocation::Searcher => {
                    self.order_storage.remove_searcher_order(&order_id)
                }
            })
            .collect::<Vec<OrderWithStorageData<AllOrders>>>();

        filled_orders.iter().for_each(|order| {
            self.notify_order_subscribers(PoolManagerUpdate::FilledOrder((
                block,
                order.order.clone()
            )));
        });
        self.order_storage.add_filled_orders(block, filled_orders);
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
                    return Ok(PoolInnerEvent::BadOrderMessages(peers));
                }

                self.notify_order_subscribers(PoolManagerUpdate::NewOrder(valid.order.clone()));
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
                    OrderValidationResults::Invalid(bad_hash.clone())
                );
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
        if let Err(e) = self.orders_subscriber_tx.send(update) {
            error!("could not send order update {:?}", e)
        }
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
                                GroupedUserOrder::Vanilla(GroupedVanillaOrder::Partial(p))
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
        self.order_hash_to_order_id.insert(hash.clone(), id);
        // nonce overlap is checked during validation so its ok we
        // don't check for duplicates
        self.address_to_orders.entry(user).or_default().push(id);
    }

    pub fn get_all_orders(&self) -> OrderSet<GroupedVanillaOrder, TopOfBlockOrder> {
        self.order_storage.get_all_orders()
    }

    pub fn start_new_block_processing(
        &mut self,
        block: u64,
        completed_orders: Vec<B256>,
        address_changes: Vec<Address>
    ) {
        tracing::info!(%block, "starting transition to new block processing");
        self.validator
            .on_new_block(block, completed_orders, address_changes);
    }

    fn finish_new_block_processing(
        &mut self,
        block: u64,
        mut completed_orders: Vec<B256>,
        address_changes: Vec<Address>
    ) {
        // deal with changed orders
        self.eoa_state_change(&address_changes);
        // deal with filled orders
        self.filled_orders(block, &completed_orders);
        // add expired orders to completed
        completed_orders.extend(self.remove_expired_orders(block));

        self.validator
            .notify_validation_on_changes(block, completed_orders, address_changes);
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
                    self.finish_new_block_processing(block, orders, addresses);
                }
                OrderValidatorRes::ValidatedOrder(next) => {
                    if let Ok(prop) = self.handle_validated_order(next) {
                        validated.push(prop);
                    }
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
