use std::{
    collections::{HashMap, HashSet},
    ops::Deref,
    pin::Pin,
    task::{Context, Poll},
    time::{Duration, SystemTime, UNIX_EPOCH}
};

use alloy_primitives::{B256, U256};
use angstrom_types::{
    orders::{OrderConversion, OrderId, OrderOrigin, PooledOrder},
    primitive::PoolId,
    rpc::{
        SignedComposableLimitOrder, SignedComposableSearcherOrder, SignedLimitOrder,
        SignedSearcherOrder
    },
    sol_bindings::grouped_orders::{
        AllOrders, GroupedComposableOrder, GroupedVanillaOrder, OrderWithStorageData, *
    }
};
use futures_util::{Stream, StreamExt};
use reth_network_peers::PeerId;
use reth_primitives::Address;
use tracing::{error, trace};
use validation::order::OrderValidatorHandle;

use crate::{config::PoolConfig, order_storage::OrderStorage, validator::PoolOrderValidator};

/// This is used to remove validated orders. During validation
/// the same check wil be ran but with more accuracy
const ETH_BLOCK_TIME: Duration = Duration::from_secs(12);

pub struct OrderIndexer<V: OrderValidatorHandle> {
    _config:                PoolConfig,
    /// order storage
    order_storage:          OrderStorage,
    /// Address to order id, used for eoa invalidation
    address_to_orders:      HashMap<Address, Vec<OrderId>>,
    /// touched addresses transition
    last_touched_addresses: HashSet<Address>,
    /// current block_number
    block_number:           u64,
    /// Order hash to order id, used for order inclusion lookups
    hash_to_order_id:       HashMap<B256, OrderId>,
    /// Orders that are being validated
    pending_order_indexing: HashMap<B256, Vec<PeerId>>,
    /// Order Validator
    validator:              PoolOrderValidator<V>
}

impl<V: OrderValidatorHandle> Deref for OrderIndexer<V> {
    type Target = OrderStorage;

    fn deref(&self) -> &Self::Target {
        &self.order_storage
    }
}

impl<V: OrderValidatorHandle> OrderIndexer<V> {
    pub fn new(validator: V, config: PoolConfig, block_number: u64) -> Self {
        Self {
            order_storage: OrderStorage::new(&config),
            block_number,
            _config: config,
            address_to_orders: HashMap::new(),
            hash_to_order_id: HashMap::new(),
            pending_order_indexing: HashMap::new(),
            last_touched_addresses: HashSet::new(),
            validator: PoolOrderValidator::new(validator)
        }
    }

    pub fn new_order(&mut self, peer_id: PeerId, origin: OrderOrigin, order: AllOrders) {
        let hash = order.order_hash();
        self.pending_order_indexing
            .entry(hash)
            .or_default()
            .push(peer_id);

        self.validator.validate_order(origin, order);
    }

    fn is_duplicate(&self, order: &AllOrders) -> bool {
        let hash = order.order_hash();
        if self.hash_to_order_id.contains_key(&hash)
            || self.pending_order_indexing.contains_key(&hash)
        {
            trace!(?hash, "got duplicate order");
            return true
        }

        false
    }

    /// used to remove orders that expire before the next ethereum block
    pub fn new_block(&mut self, block_number: u64) {
        self.block_number = block_number;
        if let Ok(time) = SystemTime::now().duration_since(UNIX_EPOCH) {
            let expiry_deadline = U256::from((time + ETH_BLOCK_TIME).as_secs()); // grab all exired hashes
            let hashes = self
                .hash_to_order_id
                .iter()
                .filter(|(_, v)| v.deadline <= expiry_deadline)
                .map(|(k, _)| *k)
                .collect::<Vec<_>>();

            // TODO: notify rpc of dead orders
            let _expired_orders = hashes
                .into_iter()
                // remove hash from id
                .map(|hash| self.hash_to_order_id.remove(&hash).unwrap())
                .inspect(|order_id| {
                    self.address_to_orders
                        .values_mut()
                        // remove from address to orders
                        .for_each(|v| v.retain(|o| o != order_id));
                })
                // remove from all underlying pools
                .filter_map(|id| match id.location {
                    angstrom_types::orders::OrderLocation::Searcher => self
                        .order_storage
                        .remove_searcher_order(&id)
                        .map(Into::into),
                    angstrom_types::orders::OrderLocation::Limit => {
                        self.order_storage.remove_limit_order(&id).map(Into::into)
                    }
                })
                .collect::<Vec<AllOrders>>();
        }
    }

    pub fn eoa_state_change(&mut self, eoas: Vec<Address>) {
        let mut rem = HashSet::new();
        eoas.into_iter()
            .filter_map(|eoa| {
                self.address_to_orders.remove(&eoa).or_else(|| {
                    rem.insert(eoa);
                    None
                })
            })
            .for_each(|order_ids| {
                order_ids.into_iter().for_each(|id| {
                    let Some(order) = (match id.location {
                        angstrom_types::orders::OrderLocation::Limit => {
                            self.order_storage.remove_limit_order(&id).map(Into::into)
                        }
                        angstrom_types::orders::OrderLocation::Searcher => self
                            .order_storage
                            .remove_searcher_order(&id)
                            .map(Into::into)
                    }) else {
                        return
                    };

                    self.validator.validate_order(OrderOrigin::Local, order);
                })
            });

        // for late updates that might need to be re validated.
        self.last_touched_addresses = rem;
    }

    pub fn finalized_block(&mut self, block: u64) {
        self.order_storage.finalized_block(block);
    }

    pub fn reorg(&mut self, orders: Vec<B256>) {
        self.order_storage
            .reorg(orders)
            .into_iter()
            .for_each(|order| self.validator.validate_order(OrderOrigin::Local, order));
    }

    /// Removes all filled orders from the pools and moves to regular pool
    pub fn filled_orders(&mut self, block: u64, orders: &[B256]) {
        if orders.is_empty() {
            return
        }

        let filled_orders = orders
            .iter()
            .filter_map(|hash| self.hash_to_order_id.remove(hash))
            .filter_map(|order_id| match order_id.location {
                angstrom_types::orders::OrderLocation::Limit => self
                    .order_storage
                    .remove_limit_order(&order_id)
                    .map(Into::into),
                angstrom_types::orders::OrderLocation::Searcher => self
                    .order_storage
                    .remove_searcher_order(&order_id)
                    .map(Into::into)
            })
            .collect::<Vec<AllOrders>>();

        self.order_storage.add_filled_orders(block, filled_orders);
    }

    fn handle_validated_order(&mut self, res: OrderWithStorageData<AllOrders>) -> eyre::Result<()> {
        if res.is_valid
            && res.valid_block == self.block_number
            && !self.last_touched_addresses.remove(&res.from())
        {
            // set tracking
            self.update_order_tracking(&res);

            // insert
            match res.order_id.location {
                angstrom_types::orders::OrderLocation::Searcher => {
                    self.order_storage.add_new_searcher_order(
                        res.try_map_inner(|inner| {
                            let AllOrders::TOB(order) = inner else { eyre::bail!("unreachable") };
                            Ok(order)
                        })
                        .expect("should be unreachable")
                    )?;
                }
                angstrom_types::orders::OrderLocation::Limit => {
                    self.order_storage.add_new_limit_order(
                        res.try_map_inner(|inner| {
                            Ok(match inner {
                                AllOrders::Partial(p) => {
                                    if p.hook_data.is_empty() {
                                        GroupedUserOrder::Vanilla(GroupedVanillaOrder::Partial(p))
                                    } else {
                                        GroupedUserOrder::Composable(
                                            GroupedComposableOrder::Partial(p)
                                        )
                                    }
                                }
                                AllOrders::KillOrFill(kof) => {
                                    if kof.hook_data.is_empty() {
                                        GroupedUserOrder::Vanilla(GroupedVanillaOrder::KillOrFill(
                                            kof
                                        ))
                                    } else {
                                        GroupedUserOrder::Composable(
                                            GroupedComposableOrder::KillOrFill(kof)
                                        )
                                    }
                                }
                                _ => eyre::bail!("unreachable")
                            })
                        })
                        .expect("should be unreachable")
                    )?;
                }
            }

            return Ok(())
        }

        // handle invalid case
        let peers = self
            .pending_order_indexing
            .remove(&res.order_hash())
            .unwrap_or_default();
        // TODO: broadcast bad peers

        Ok(())
    }

    fn update_order_tracking(&mut self, order: &OrderWithStorageData<AllOrders>) {
        let hash = order.order_hash();
        let user = order.from();
        let id: OrderId = order.order_id;

        self.pending_order_indexing.remove(&hash);
        self.hash_to_order_id.insert(hash, id);
        // nonce overlap is checked during validation so its ok we
        // don't check for duplicates
        self.address_to_orders.entry(user).or_default().push(id);
    }
}

impl<V> Stream for OrderIndexer<V>
where
    V: OrderValidatorHandle
{
    type Item = Vec<()>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let validated = Vec::new();
        while let Poll::Ready(Some(next)) = self.validator.poll_next_unpin(cx) {
            if let Ok(prop) = self.handle_validated_order(next) {
                // validated.push(prop);
            }
        }

        if validated.is_empty() {
            Poll::Pending
        } else {
            Poll::Ready(Some(validated))
        }
    }
}

pub enum PoolInnerEvent<L, CL, S, CS> {
    Propagation(OrdersToPropagate<L, CL, S, CS>),
    BadOrderMessages(Vec<PeerId>)
}

impl<L, CL, S, CS> PoolInnerEvent<L, CL, S, CS> {
    fn from_limit(order: OrderOrPeers<L>) -> Option<Self> {
        match order {
            OrderOrPeers::None => None,
            OrderOrPeers::Order(o) => {
                Some(PoolInnerEvent::Propagation(OrdersToPropagate::Limit(o?)))
            }
            OrderOrPeers::Peers(p) => Some(PoolInnerEvent::BadOrderMessages(p))
        }
    }

    fn from_searcher(order: OrderOrPeers<S>) -> Option<Self> {
        match order {
            OrderOrPeers::None => None,
            OrderOrPeers::Order(o) => {
                Some(PoolInnerEvent::Propagation(OrdersToPropagate::Searcher(o?)))
            }
            OrderOrPeers::Peers(p) => Some(PoolInnerEvent::BadOrderMessages(p))
        }
    }

    fn from_composable_limit(order: OrderOrPeers<CL>) -> Option<Self> {
        match order {
            OrderOrPeers::None => None,
            OrderOrPeers::Order(o) => {
                Some(PoolInnerEvent::Propagation(OrdersToPropagate::ComposableLimit(o?)))
            }
            OrderOrPeers::Peers(p) => Some(PoolInnerEvent::BadOrderMessages(p))
        }
    }

    fn from_composable_searcher(order: OrderOrPeers<CS>) -> Option<Self> {
        match order {
            OrderOrPeers::None => None,
            OrderOrPeers::Order(o) => {
                Some(PoolInnerEvent::Propagation(OrdersToPropagate::ComposableSearcher(o?)))
            }
            OrderOrPeers::Peers(p) => Some(PoolInnerEvent::BadOrderMessages(p))
        }
    }
}

pub enum OrdersToPropagate<L, CL, S, CS> {
    Limit(L),
    ComposableLimit(CL),
    Searcher(S),
    ComposableSearcher(CS)
}

enum OrderOrPeers<O> {
    Order(Option<O>),
    Peers(Vec<PeerId>),
    None
}

impl<L, CL, S, CS> OrdersToPropagate<L, CL, S, CS>
where
    L: OrderConversion<Order = SignedLimitOrder>,
    CL: OrderConversion<Order = SignedComposableLimitOrder>,
    S: OrderConversion<Order = SignedSearcherOrder>,
    CS: OrderConversion<Order = SignedComposableSearcherOrder>
{
    pub fn into_pooled(self) -> PooledOrder {
        match self {
            Self::Limit(l) => PooledOrder::Limit(l.to_signed()),
            Self::Searcher(s) => PooledOrder::Searcher(s.to_signed()),
            Self::ComposableLimit(cl) => PooledOrder::ComposableLimit(cl.to_signed()),
            Self::ComposableSearcher(cs) => PooledOrder::ComposableSearcher(cs.to_signed())
        }
    }
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
