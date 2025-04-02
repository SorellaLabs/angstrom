use std::{
    collections::{HashMap, HashSet},
    time::{Duration, SystemTime, UNIX_EPOCH}
};

use alloy::primitives::{Address, B256, U256};
use angstrom_types::{
    orders::{OrderId, OrderLocation},
    primitive::{PeerId, PoolId},
    sol_bindings::{
        RawPoolOrder, ext::grouped_orders::AllOrders, grouped_orders::OrderWithStorageData
    }
};
use validation::order::OrderValidatorHandle;

use crate::{
    order_indexer::InnerCancelOrderRequest, order_storage::OrderStorage, validator::OrderValidator
};

/// This is used to remove validated orders. During validation
/// the same check wil be ran but with more accuracy
const ETH_BLOCK_TIME: Duration = Duration::from_secs(12);
const MAX_NEW_ORDER_DELAY_PROPAGATION: u64 = 7000;

/// Used as a storage of order hashes to order ids of validated and pending
/// validation orders.
#[derive(Default)]
pub struct OrderTracker {
    pub(super) address_to_orders:      HashMap<Address, Vec<OrderId>>,
    /// current block_number
    /// Order hash to order id, used for order inclusion lookups
    pub(super) order_hash_to_order_id: HashMap<B256, OrderId>,
    /// Used to get trigger reputation side-effects on network order submission
    pub(super) order_hash_to_peer_id:  HashMap<B256, Vec<PeerId>>,
    /// Used to avoid unnecessary computation on order spam
    pub(super) seen_invalid_orders:    HashSet<B256>,
    /// Used to protect against late order propagation
    pub(super) cancelled_orders:       HashMap<B256, InnerCancelOrderRequest>
}

impl OrderTracker {
    pub fn new() -> Self {
        Self::default()
    }

    #[inline(always)]
    pub fn clear_invalid(&mut self) {
        self.seen_invalid_orders.clear()
    }

    #[inline(always)]
    pub fn is_seen_invalid(&self, order_hash: &B256) -> bool {
        self.seen_invalid_orders.contains(order_hash)
    }

    #[inline(always)]
    pub fn is_cancelled(&self, order_hash: &B256) -> bool {
        self.cancelled_orders.contains_key(order_hash)
    }

    pub fn is_valid_cancel(&self, order: &B256, order_addr: Address) -> bool {
        let Some(req) = self.cancelled_orders.get(order) else { return false };
        req.from == order_addr
    }

    pub fn is_duplicate(&self, hash: &B256) -> bool {
        self.order_hash_to_order_id.contains_key(hash) || self.seen_invalid_orders.contains(hash)
    }

    pub fn track_peer_id(&mut self, hash: B256, peer_id: Option<PeerId>) {
        if let Some(peer) = peer_id {
            self.order_hash_to_peer_id
                .entry(hash)
                .or_default()
                .push(peer);
        }
    }

    pub fn invalid_verification(&mut self, hash: B256) -> Vec<PeerId> {
        self.seen_invalid_orders.insert(hash);

        self.order_hash_to_peer_id.remove(&hash).unwrap_or_default()
    }

    pub fn remove_expired_orders(
        &mut self,
        block_number: u64,
        storage: &OrderStorage
    ) -> Vec<B256> {
        let time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        let expiry_deadline = U256::from((time + ETH_BLOCK_TIME).as_secs()); // grab all expired hashes

        // clear canceled order cache
        self.cancelled_orders
            .retain(|_, req| req.deadline > expiry_deadline);

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

        hashes
            .iter()
            // remove hash from id
            .map(|hash| self.order_hash_to_order_id.remove(hash).unwrap())
            // remove from all underlying pools
            .for_each(|id| {
                self.address_to_orders
                    .values_mut()
                    // remove from address to orders
                    .for_each(|v| v.retain(|o| o != &id));
                match id.location {
                    OrderLocation::Searcher => storage.remove_searcher_order(&id),
                    OrderLocation::Limit => storage.remove_limit_order(&id)
                };
            });

        hashes
    }

    pub fn filled_orders<'a>(
        &'a mut self,
        orders: &'a [B256],
        storage: &'a OrderStorage
    ) -> impl Iterator<Item = OrderWithStorageData<AllOrders>> + 'a {
        orders
            .iter()
            .filter_map(|hash| self.order_hash_to_order_id.remove(hash))
            .filter_map(move |order_id| match order_id.location {
                OrderLocation::Limit => storage.remove_limit_order(&order_id),
                OrderLocation::Searcher => storage.remove_searcher_order(&order_id)
            })
    }

    pub fn park_orders(&mut self, txes: &[B256], storage: &OrderStorage) {
        let order_info = txes
            .iter()
            .filter_map(|tx_hash| self.order_hash_to_order_id.get(tx_hash))
            .collect::<Vec<_>>();
        storage.park_orders(order_info);
    }

    pub fn new_valid_order(&mut self, hash: &B256, user: Address, id: OrderId) {
        self.order_hash_to_peer_id.remove(hash);
        self.order_hash_to_order_id.insert(*hash, id);
        // nonce overlap is checked during validation so its ok we
        // don't check for duplicates
        self.address_to_orders.entry(user).or_default().push(id);
    }

    fn cancel_with_next_block_deadline(&mut self, from: Address, order_hash: &B256) {
        let deadline = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + MAX_NEW_ORDER_DELAY_PROPAGATION * ETH_BLOCK_TIME.as_secs();
        self.insert_cancel_with_deadline(from, order_hash, Some(U256::from(deadline)));
    }

    fn insert_cancel_with_deadline(
        &mut self,
        from: Address,
        order_hash: &B256,
        deadline: Option<U256>
    ) {
        let valid_until = deadline.map_or_else(
            || {
                // if no deadline is provided the cancellation request is valid until block
                // transition
                U256::from(
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_secs()
                )
            },
            |deadline| deadline
        );
        self.cancelled_orders
            .insert(*order_hash, InnerCancelOrderRequest { from, deadline: valid_until });
    }

    pub fn cancel_order(
        &mut self,
        from: Address,
        hash: B256,
        storage: &OrderStorage
    ) -> Option<PoolId> {
        self.order_hash_to_order_id
            .remove(&hash)
            .and_then(|v| storage.cancel_order(&v))
            .map(|order| {
                self.order_hash_to_order_id.remove(&order.order_hash());
                self.order_hash_to_peer_id.remove(&order.order_hash());
                self.insert_cancel_with_deadline(order.from(), &hash, order.deadline());

                order.pool_id
            })
            .or_else(|| {
                // in the case we haven't index the order yet, we are going to add it
                // in the cancel to register
                self.cancel_with_next_block_deadline(from, &hash);

                None
            })
    }

    pub fn pending_orders_for_address<F>(
        &self,
        address: Address,
        storage: &OrderStorage,
        f: F
    ) -> Vec<OrderWithStorageData<AllOrders>>
    where
        F: Fn(&OrderId, &OrderStorage) -> Option<OrderWithStorageData<AllOrders>>
    {
        self.address_to_orders
            .get(&address)
            .map(|order_ids| {
                order_ids
                    .iter()
                    .filter_map(|order_id| f(order_id, storage))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn eoa_state_changes<F, V>(
        &mut self,
        eoas: &[Address],
        indexer: &OrderStorage,
        validator: &mut OrderValidator<V>,
        f: F
    ) where
        F: Fn(&OrderId, &OrderStorage, &mut OrderValidator<V>),
        V: OrderValidatorHandle<Order = AllOrders>
    {
        eoas.iter()
            .filter_map(|eoa| self.address_to_orders.remove(eoa))
            .flatten()
            .for_each(|id| f(&id, indexer, validator));
    }
}
