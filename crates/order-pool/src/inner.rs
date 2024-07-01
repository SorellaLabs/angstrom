use std::{
    collections::{HashMap, HashSet},
    pin::Pin,
    task::{Context, Poll},
    time::{Duration, SystemTime, UNIX_EPOCH}
};

use alloy_primitives::B256;
use angstrom_types::{
    orders::{
        OrderConversion, OrderId, OrderLocation, OrderOrigin, OrderPriorityData,
        OrderValidationOutcome, PoolOrder, PooledComposableOrder, PooledLimitOrder, PooledOrder,
        PooledSearcherOrder, SearcherPriorityData, ValidationResults
    },
    primitive::PoolId,
    rpc::{
        SignedComposableLimitOrder, SignedComposableSearcherOrder, SignedLimitOrder,
        SignedSearcherOrder
    }
};
use futures_util::{Stream, StreamExt};
use reth_network_peers::PeerId;
use reth_primitives::{Address, U256};
use tokio::sync::mpsc;
use tracing::{error, trace, warn};
use validation::order::OrderValidatorHandle;

use crate::{
    common::{Order, ValidOrder},
    config::PoolConfig,
    finalization_pool::FinalizationPool,
    limit::LimitOrderPool,
    searcher::SearcherPool,
    subscriptions::OrderPoolSubscriptions,
    validator::PoolOrderValidator,
    OrderSet
};

/// This is used to remove validated orders. During validation
/// the same check wil be ran but with more accuracy
const ETH_BLOCK_TIME: Duration = Duration::from_secs(12);

pub struct OrderPoolInner<L, CL, S, CS, V>
where
    L: PooledLimitOrder,
    CL: PooledComposableOrder + PooledLimitOrder,
    S: PooledSearcherOrder,
    CS: PooledComposableOrder + PooledSearcherOrder,
    V: OrderValidatorHandle
{
    limit_pool:             LimitOrderPool<L, CL>,
    searcher_pool:          SearcherPool<S, CS>,
    finalization_pool:      FinalizationPool<L, CL, S, CS>,
    _config:                PoolConfig,
    /// Address to order id, used for eoa invalidation
    address_to_orders:      HashMap<Address, Vec<OrderId>>,
    /// touched addresses transition
    last_touched_addresses: HashSet<Address>,
    /// current block_number
    block_number:           u64,
    /// Order hash to order id, used for order inclusion lookups
    hash_to_order_id:       HashMap<B256, OrderId>,
    /// Orders that are being validated
    pending_orders:         HashMap<B256, Vec<PeerId>>,
    /// Order Validator
    validator:              PoolOrderValidator<L, CL, S, CS, V>,
    /// handles sending out subscriptions
    subscriptions:          OrderPoolSubscriptions<L, CL, S, CS>
}

impl<L, CL, S, CS, V> OrderPoolInner<L, CL, S, CS, V>
where
    L: PooledLimitOrder<ValidationData = OrderPriorityData>,
    CL: PooledComposableOrder + PooledLimitOrder<ValidationData = OrderPriorityData>,

    S: PooledSearcherOrder<ValidationData = SearcherPriorityData>,
    CS: PooledComposableOrder + PooledSearcherOrder<ValidationData = SearcherPriorityData>,
    V: OrderValidatorHandle<
        LimitOrder = L,
        SearcherOrder = S,
        ComposableLimitOrder = CL,
        ComposableSearcherOrder = CS
    >
{
    pub fn new(validator: V, config: PoolConfig, block_number: u64) -> Self {
        Self {
            block_number,
            limit_pool: LimitOrderPool::new(&config.ids, None),
            searcher_pool: SearcherPool::new(&config.ids, None),
            finalization_pool: FinalizationPool::new(),
            _config: config,
            address_to_orders: HashMap::new(),
            hash_to_order_id: HashMap::new(),
            pending_orders: HashMap::new(),
            last_touched_addresses: HashSet::new(),
            validator: PoolOrderValidator::new(validator),
            subscriptions: OrderPoolSubscriptions::new()
        }
    }

    pub fn new_limit_order(&mut self, peer_id: PeerId, origin: OrderOrigin, order: L) {
        if !self.is_duplicate(&order) {
            self.pending_orders.insert(order.hash(), vec![peer_id]);
            self.validator.validate_order(origin, order);
        } else {
            // track other peers in-case of invalid messages. This allows us to slash all
            // invalid messages sent
            self.pending_orders
                .get_mut(&order.hash())
                .unwrap()
                .push(peer_id);
        }
    }

    pub fn new_composable_limit(&mut self, peer_id: PeerId, origin: OrderOrigin, order: CL) {
        if !self.is_duplicate(&order) {
            self.pending_orders.insert(order.hash(), vec![peer_id]);
            self.validator.validate_composable_order(origin, order);
        } else {
            // track other peers in-case of invalid messages. This allows us to slash all
            // invalid messages sent
            self.pending_orders
                .get_mut(&order.hash())
                .unwrap()
                .push(peer_id);
        }
    }

    pub fn new_searcher_order(&mut self, peer_id: PeerId, origin: OrderOrigin, order: S) {
        if !self.is_duplicate(&order) {
            self.pending_orders.insert(order.hash(), vec![peer_id]);
            self.validator.validate_searcher_order(origin, order)
        } else {
            // track other peers in-case of invalid messages. This allows us to slash all
            // invalid messages sent
            self.pending_orders
                .get_mut(&order.hash())
                .unwrap()
                .push(peer_id);
        }
    }

    pub fn new_composable_searcher_order(
        &mut self,
        peer_id: PeerId,
        origin: OrderOrigin,
        order: CS
    ) {
        if !self.is_duplicate(&order) {
            self.pending_orders.insert(order.hash(), vec![peer_id]);
            self.validator
                .validate_composable_searcher_order(origin, order)
        } else {
            // track other peers in-case of invalid messages. This allows us to slash all
            // invalid messages sent
            self.pending_orders
                .get_mut(&order.hash())
                .unwrap()
                .push(peer_id);
        }
    }

    fn is_duplicate<O: PoolOrder>(&self, order: &O) -> bool {
        let hash = order.hash();
        if self.hash_to_order_id.contains_key(&hash) || self.pending_orders.contains_key(&hash) {
            trace!(?hash, "got duplicate order");
            return true
        }

        false
    }

    pub fn subscribe_new_orders(&mut self, tx: mpsc::Sender<Order<L, CL, S, CS>>) {
        self.subscriptions.subscribe_new_orders(tx);
    }

    pub fn subscribe_finalized_orders(&mut self, tx: mpsc::Sender<Vec<Order<L, CL, S, CS>>>) {
        self.subscriptions.subscribe_finalized_orders(tx);
    }

    pub fn subscribe_filled_orders(&mut self, tx: mpsc::Sender<Vec<Order<L, CL, S, CS>>>) {
        tracing::debug!("new sub for filled orders");
        self.subscriptions.subscribe_filled_orders(tx);
    }

    pub fn subscribe_expired_orders(&mut self, tx: mpsc::Sender<Vec<Order<L, CL, S, CS>>>) {
        self.subscriptions.subscribe_expired_orders(tx);
    }

    pub fn fetch_vanilla_orders(&self) -> OrderSet<L, S> {
        let limit = self.limit_pool.fetch_all_vanilla_orders();
        let searcher = self.searcher_pool.get_winning_orders_vanilla();

        OrderSet { limit, searcher }
    }

    pub fn fetch_composable_orders(&self) -> OrderSet<CL, CS> {
        let limit = self.limit_pool.fetch_all_composable_orders();
        let searcher = self.searcher_pool.get_winning_orders_composable();

        OrderSet { limit, searcher }
    }

    /// used to remove orders that expire before the next ethereum block
    pub fn new_block(&mut self, block_number: u64) {
        self.block_number = block_number;
        if let Ok(time) = SystemTime::now().duration_since(UNIX_EPOCH) {
            let expiry_deadline = U256::from((time + ETH_BLOCK_TIME).as_secs());
            // grab all exired hashes
            let hashes = self
                .hash_to_order_id
                .iter()
                .filter(|(_, v)| v.deadline <= expiry_deadline)
                .map(|(k, _)| *k)
                .collect::<Vec<_>>();

            let expired_orders = hashes
                .into_iter()
                // remove hash from id
                .map(|hash| self.hash_to_order_id.remove(&hash).unwrap())
                .map(|order_id| {
                    self.address_to_orders
                        .values_mut()
                        // remove from address to orders
                        .for_each(|v| v.retain(|o| o != &order_id));
                    order_id
                })
                // remove from all underlying pools
                .filter_map(|id| match id.location {
                    OrderLocation::LimitParked | OrderLocation::LimitPending => self
                        .limit_pool
                        .remove_limit_order(&id)
                        .map(|o| Order::add_limit(o.order)),
                    OrderLocation::VanillaSearcher => self
                        .searcher_pool
                        .remove_searcher_order(&id)
                        .ok()
                        .map(|o| Order::add_searcher(o.order)),
                    OrderLocation::Composable => self
                        .limit_pool
                        .remove_composable_limit_order(&id)
                        .map(|o| Order::add_composable_limit(o.order)),
                    OrderLocation::ComposableSearcher => self
                        .searcher_pool
                        .remove_composable_searcher_order(&id)
                        .ok()
                        .map(|o| Order::add_composable_searcher(o.order))
                })
                .collect::<Vec<_>>();

            self.subscriptions.expired_orders(expired_orders);
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
                order_ids.into_iter().for_each(|id| match id.location {
                    OrderLocation::Composable => {
                        if let Some(order) = self.limit_pool.remove_composable_limit_order(&id) {
                            self.validator
                                .validate_composable_order(OrderOrigin::Local, order.order);
                        }
                    }
                    OrderLocation::LimitParked | OrderLocation::LimitPending => {
                        if let Some(order) = self.limit_pool.remove_limit_order(&id) {
                            self.validator
                                .validate_order(OrderOrigin::Local, order.order);
                        }
                    }

                    OrderLocation::VanillaSearcher => {
                        if let Ok(order) = self.searcher_pool.remove_searcher_order(&id) {
                            self.validator
                                .validate_searcher_order(OrderOrigin::Local, order.order);
                        }
                    }
                    OrderLocation::ComposableSearcher => {
                        if let Ok(order) = self.searcher_pool.remove_composable_searcher_order(&id)
                        {
                            self.validator
                                .validate_composable_searcher_order(OrderOrigin::Local, order.order)
                        }
                    }
                })
            });

        // for late updates that might need to be re validated.
        self.last_touched_addresses = rem;
    }

    pub fn finalized_block(&mut self, block: u64) {
        self.subscriptions
            .finalized_orders(self.finalization_pool.finalized(block))
    }

    pub fn reorg(&mut self, orders: Vec<B256>) {
        self.finalization_pool
            .reorg(orders)
            .for_each(|order| match order {
                Order::ComposableSearcher(cs) => self
                    .validator
                    .validate_composable_searcher_order(OrderOrigin::Local, cs),
                Order::ComposableLimit(cl) => self
                    .validator
                    .validate_composable_order(OrderOrigin::Local, cl),
                Order::Limit(l) => self.validator.validate_order(OrderOrigin::Local, l),
                Order::Searcher(s) => self
                    .validator
                    .validate_searcher_order(OrderOrigin::Local, s)
            });
    }

    /// Removes all filled orders from the pools
    pub fn filled_orders(&mut self, block: u64, orders: &[B256]) {
        let filled = orders
            .iter()
            .filter_map(|order_hash| {
                let order_id = self.hash_to_order_id.remove(order_hash)?;
                let loc = order_id.location;
                match loc {
                    OrderLocation::Composable => self
                        .limit_pool
                        .remove_composable_limit_order(&order_id)
                        .map(|o| o.order)
                        .map(Order::add_composable_limit),
                    OrderLocation::LimitParked | OrderLocation::LimitPending => self
                        .limit_pool
                        .remove_limit_order(&order_id)
                        .map(|o| o.order)
                        .map(Order::add_limit),
                    OrderLocation::VanillaSearcher => self
                        .searcher_pool
                        .remove_searcher_order(&order_id)
                        .map_err(|e| {
                            error!("{e:?}");
                            e
                        })
                        .ok()
                        .map(|o| o.order)
                        .map(Order::add_searcher),
                    OrderLocation::ComposableSearcher => self
                        .searcher_pool
                        .remove_composable_searcher_order(&order_id)
                        .map_err(|e| {
                            error!("{e:?}");
                            e
                        })
                        .ok()
                        .map(|o| o.order)
                        .map(Order::add_composable_searcher)
                }
            })
            .collect::<Vec<_>>();

        self.subscriptions.filled_orders(filled.clone());
        self.finalization_pool.new_orders(block, filled);
    }

    fn handle_validated_order(
        &mut self,
        res: ValidationResults<L, CL, S, CS>
    ) -> Option<PoolInnerEvent<L, CL, S, CS>> {
        match res {
            ValidationResults::Limit(order) => {
                PoolInnerEvent::from_limit(self.handle_validation_results(
                    order,
                    |this, order| {
                        this.subscriptions
                            .new_order(Order::Limit(order.order.clone()));

                        if let Err(e) = this.limit_pool.add_limit_order(order) {
                            error!(error=%e, "failed to add order to limit pool");
                        }
                    },
                    |this, order| this.validator.validate_order(OrderOrigin::External, order)
                ))
            }

            ValidationResults::Searcher(order) => {
                PoolInnerEvent::from_searcher(self.handle_validation_results(
                    order,
                    |this, order| {
                        this.subscriptions
                            .new_order(Order::Searcher(order.order.clone()));

                        if let Err(e) = this.searcher_pool.add_searcher_order(order) {
                            error!(error=%e, "failed to add order to searcher pool");
                        }
                    },
                    |this, order| {
                        this.validator
                            .validate_searcher_order(OrderOrigin::External, order)
                    }
                ))
            }
            ValidationResults::ComposableLimit(order) => {
                PoolInnerEvent::from_composable_limit(self.handle_validation_results(
                    order,
                    |this, order| {
                        this.subscriptions
                            .new_order(Order::ComposableLimit(order.order.clone()));

                        if let Err(e) = this.limit_pool.add_composable_order(order) {
                            error!(error=%e, "failed to add order to limit pool");
                        }
                    },
                    |this, order| {
                        this.validator
                            .validate_composable_order(OrderOrigin::External, order)
                    }
                ))
            }
            ValidationResults::ComposableSearcher(order) => {
                PoolInnerEvent::from_composable_searcher(self.handle_validation_results(
                    order,
                    |this, order| {
                        this.subscriptions
                            .new_order(Order::ComposableSearcher(order.order.clone()));

                        if let Err(e) = this.searcher_pool.add_composable_searcher_order(order) {
                            error!(error=%e,"failed to add order to searcher pool");
                        }
                    },
                    |this, order| {
                        this.validator
                            .validate_composable_searcher_order(OrderOrigin::External, order)
                    }
                ))
            }
        }
    }

    fn handle_validation_results<O: PoolOrder>(
        &mut self,
        order: OrderValidationOutcome<O>,
        insert: impl FnOnce(&mut Self, ValidOrder<O>),
        revalidate: impl FnOnce(&mut Self, O)
    ) -> OrderOrPeers<O> {
        match order {
            OrderValidationOutcome::Valid { order, propagate, block_number } => {
                // check against current block to see if there is possible state reminance.
                if block_number + 1 == self.block_number
                    && self.last_touched_addresses.remove(&order.from())
                {
                    tracing::debug!(
                        ?order,
                        "order was validated on prev block but had a state change occur. \
                         revalidating"
                    );
                    revalidate(self, order.order);
                    return OrderOrPeers::None
                }
                let res = propagate.then_some(order.order.clone());
                self.update_order_tracking(order.clone());
                insert(self, order);

                OrderOrPeers::Order(res)
            }
            OrderValidationOutcome::Invalid(order, e) => {
                warn!(?order, %e, "invalid order");
                let peers = self
                    .pending_orders
                    .remove(&order.hash())
                    .unwrap_or_default();

                OrderOrPeers::Peers(peers)
            }
            OrderValidationOutcome::Error(hash, e) => {
                error!(?hash, %e, "error validating order");

                OrderOrPeers::None
            }
        }
    }

    fn update_order_tracking<O: PoolOrder>(&mut self, order: ValidOrder<O>) {
        let hash = order.hash();
        let user = order.from();
        let id: OrderId = order.into();

        self.pending_orders.remove(&hash);
        self.hash_to_order_id.insert(hash, id);
        // nonce overlap is checked during validation so its ok we
        // don't check for duplicates
        self.address_to_orders.entry(user).or_default().push(id);
    }
}

impl<L, CL, S, CS, V> Stream for OrderPoolInner<L, CL, S, CS, V>
where
    L: PooledLimitOrder<ValidationData = OrderPriorityData>,
    CL: PooledComposableOrder + PooledLimitOrder<ValidationData = OrderPriorityData>,

    S: PooledSearcherOrder<ValidationData = SearcherPriorityData>,
    CS: PooledComposableOrder + PooledSearcherOrder<ValidationData = SearcherPriorityData>,
    V: OrderValidatorHandle<
        LimitOrder = L,
        SearcherOrder = S,
        ComposableLimitOrder = CL,
        ComposableSearcherOrder = CS
    >
{
    type Item = Vec<PoolInnerEvent<L, CL, S, CS>>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut validated = Vec::new();
        while let Poll::Ready(Some(next)) = self.validator.poll_next_unpin(cx) {
            if let Some(prop) = self.handle_validated_order(next) {
                validated.push(prop);
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
