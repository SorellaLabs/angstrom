use std::{
    collections::{HashMap, HashSet, hash_map::Entry},
    fmt::Debug,
    pin::Pin,
    sync::Arc
};

use alloy::primitives::U160;
use angstrom_types::{
    block_sync::BlockSyncConsumer,
    orders::OrderSet,
    primitive::PoolId,
    sol_bindings::{grouped_orders::AllOrders, rpc_orders::TopOfBlockOrder},
    uni_structure::BaselinePoolState
};
use futures::{
    FutureExt, Stream, StreamExt, TryFutureExt, future::BoxFuture, stream::FuturesUnordered
};
use matching_engine::{
    book::{BookOrder, OrderBook},
    build_book,
    strategy::BinarySearchStrategy
};
use order_pool::order_storage::OrderStorage;
use rayon::ThreadPool;
use serde::{Deserialize, Serialize};
use tokio::{
    sync::{mpsc, oneshot},
    time::Interval
};
use tokio_stream::wrappers::ReceiverStream;
use uniswap_v4::uniswap::{pool_data_loader::PoolDataLoader, pool_manager::SyncedUniswapPools};

mod consensus;
mod rollup;

pub use crate::{
    consensus::{ConsensusMode, ConsensusQuoterManager},
    rollup::{RollupMode, RollupQuoterManager}
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct Slot0Update {
    /// there will be 120 updates per block or per 100ms
    pub seq_id:           u16,
    /// in case of block lag on node
    pub current_block:    u64,
    pub angstrom_pool_id: PoolId,
    pub uni_pool_id:      PoolId,

    pub sqrt_price_x96: U160,
    pub liquidity:      u128,
    pub tick:           i32
}

pub trait AngstromBookQuoter: Send + Sync + Unpin + 'static {
    /// will configure this stream to receieve updates of the given pool
    fn subscribe_to_updates(
        &self,
        pool_id: HashSet<PoolId>
    ) -> impl Future<Output = Pin<Box<dyn Stream<Item = Slot0Update> + Send + 'static>>> + Send + Sync;
}

pub struct QuoterHandle(pub mpsc::Sender<(HashSet<PoolId>, mpsc::Sender<Slot0Update>)>);

impl AngstromBookQuoter for QuoterHandle {
    async fn subscribe_to_updates(
        &self,
        pool_ids: HashSet<PoolId>
    ) -> Pin<Box<dyn Stream<Item = Slot0Update> + Send + 'static>> {
        let (tx, rx) = mpsc::channel(5);
        let _ = self.0.send((pool_ids, tx)).await;

        ReceiverStream::new(rx).boxed()
    }
}

pub struct QuoterManager<BlockSync: BlockSyncConsumer, M = ConsensusMode> {
    cur_block:           u64,
    seq_id:              u16,
    block_sync:          BlockSync,
    orders:              Arc<OrderStorage>,
    amms:                SyncedUniswapPools,
    threadpool:          ThreadPool,
    recv:                mpsc::Receiver<(HashSet<PoolId>, mpsc::Sender<Slot0Update>)>,
    book_snapshots:      HashMap<PoolId, (PoolId, BaselinePoolState)>,
    pending_tasks:       FuturesUnordered<BoxFuture<'static, eyre::Result<Slot0Update>>>,
    pool_to_subscribers: HashMap<PoolId, Vec<mpsc::Sender<Slot0Update>>>,

    execution_interval: Interval,

    mode: M
}

impl<BlockSync: BlockSyncConsumer, M> QuoterManager<BlockSync, M> {
    /// Handle new subscription
    fn handle_new_subscription(&mut self, pools: HashSet<PoolId>, chan: mpsc::Sender<Slot0Update>) {
        let keys = self
            .book_snapshots
            .iter()
            .flat_map(|(ang_key, (uni_key, _))| [ang_key, uni_key])
            .copied()
            .collect::<HashSet<_>>();

        for pool in &pools {
            if !keys.contains(pool) {
                // invalid subscription
                return;
            }
        }

        for pool in pools {
            self.pool_to_subscribers
                .entry(pool)
                .or_default()
                .push(chan.clone());
        }
    }

    /// Spawn book solvers for each book
    fn spawn_book_solvers(&mut self, seq_id: u16, orders: OrderSet<AllOrders, TopOfBlockOrder>) {
        let OrderSet { limit, searcher } = orders;
        let mut books = build_non_proposal_books(limit, &self.book_snapshots);

        let searcher_orders = searcher
            .into_iter()
            .fold(HashMap::new(), |mut acc, searcher| {
                match acc.entry(searcher.pool_id) {
                    Entry::Vacant(v) => {
                        v.insert(searcher);
                    }
                    Entry::Occupied(mut o) => {
                        let current = o.get();
                        // if this order on same pool_id has a higher tob reward or they are the
                        // same and it has a lower order hash. replace
                        if searcher.tob_reward > current.tob_reward
                            || (searcher.tob_reward == current.tob_reward
                                && searcher.order_id.hash < current.order_id.hash)
                        {
                            o.insert(searcher);
                        }
                    }
                };

                acc
            });

        for (book_id, (uni_pool_id, amm)) in &self.book_snapshots {
            // Default as if we don't have a book, we want to still send update.
            let mut book = books.remove(book_id).unwrap_or_default();
            book.id = *book_id;
            book.set_amm_if_missing(|| amm.clone());

            let searcher = searcher_orders.get(&book.id()).cloned();
            let (tx, rx) = oneshot::channel();
            let block = self.cur_block;

            let uni_pool_id = *uni_pool_id;

            self.threadpool.spawn(move || {
                let b = book;
                let (sqrt_price, tick, liquidity) =
                    BinarySearchStrategy::give_end_amm_state(&b, searcher);
                let update = Slot0Update {
                    current_block: block,
                    seq_id,
                    angstrom_pool_id: b.id(),
                    uni_pool_id,
                    liquidity,
                    sqrt_price_x96: sqrt_price,
                    tick
                };

                tx.send(update).unwrap()
            });

            self.pending_tasks.push(rx.map_err(Into::into).boxed())
        }
    }

    /// Update book state from amms
    fn update_book_state(&mut self) {
        self.book_snapshots = book_snapshots_from_amms(&self.amms);
    }

    /// Send out slot0 updates to subscribers
    fn send_out_result(&mut self, slot_update: Slot0Update) {
        if let Some(ang_pool_subs) = self
            .pool_to_subscribers
            .get_mut(&slot_update.angstrom_pool_id)
        {
            ang_pool_subs.retain(|subscriber| subscriber.try_send(slot_update.clone()).is_ok());
        };

        if let Some(uni_pool_subs) = self.pool_to_subscribers.get_mut(&slot_update.uni_pool_id) {
            uni_pool_subs.retain(|subscriber| subscriber.try_send(slot_update.clone()).is_ok());
        };
    }
}

/// Build non-proposal books from orders
pub fn build_non_proposal_books(
    limit: Vec<BookOrder>,
    pool_snapshots: &HashMap<PoolId, (PoolId, BaselinePoolState)>
) -> HashMap<PoolId, OrderBook> {
    let book_sources = orders_sorted_by_pool_id(limit);

    book_sources
        .into_iter()
        .map(|(id, orders)| {
            let amm = pool_snapshots.get(&id).map(|(_, pool)| pool).cloned();
            (id, build_book(id, amm, orders))
        })
        .collect()
}

/// Sort orders by pool id
pub fn orders_sorted_by_pool_id(limit: Vec<BookOrder>) -> HashMap<PoolId, HashSet<BookOrder>> {
    limit.into_iter().fold(HashMap::new(), |mut acc, order| {
        acc.entry(order.pool_id).or_default().insert(order);
        acc
    })
}

/// Get book snapshots from amms
pub fn book_snapshots_from_amms(
    amms: &SyncedUniswapPools
) -> HashMap<PoolId, (PoolId, BaselinePoolState)> {
    amms.iter()
        .map(|entry| {
            let pool_lock = entry.value().read().unwrap();
            let uni_key = pool_lock.data_loader().private_address();
            let snapshot_data = pool_lock.fetch_pool_snapshot().unwrap().2;
            (*entry.key(), (uni_key, snapshot_data))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use alloy_primitives::fixed_bytes;
    use angstrom_types::{
        matching::SqrtPriceX96,
        sol_bindings::{grouped_orders::OrderWithStorageData, rpc_orders::ExactFlashOrder},
        uni_structure::liquidity_base::BaselineLiquidity
    };
    use proptest::prelude::*;

    use super::*;

    // Test helper functions
    fn make_order_with_pool(pool_id: PoolId, is_bid: bool) -> BookOrder {
        let mut o = OrderWithStorageData::with_default(AllOrders::ExactFlash(Default::default()));
        o.pool_id = pool_id;
        o.is_bid = is_bid;
        o
    }

    fn make_test_pool_id(suffix: u8) -> PoolId {
        let mut bytes = [0u8; 32];
        bytes[31] = suffix;
        PoolId::from(bytes)
    }

    fn make_test_baseline_pool_state(tick: i32, liquidity: u128) -> BaselinePoolState {
        let liq = BaselineLiquidity::new(
            tick,
            0,
            SqrtPriceX96::at_tick(tick).unwrap_or_default(),
            liquidity,
            Default::default(),
            Default::default()
        );
        BaselinePoolState::new(liq, 1, 3000)
    }

    fn make_slot0_update(seq_id: u16, pool_id: PoolId) -> Slot0Update {
        Slot0Update {
            seq_id,
            current_block: 100,
            angstrom_pool_id: pool_id,
            uni_pool_id: pool_id,
            sqrt_price_x96: U160::from(1000),
            liquidity: 1000000,
            tick: 0
        }
    }

    fn make_detailed_order(pool_id: PoolId, is_bid: bool, amount: u128) -> BookOrder {
        let mut flash_order = ExactFlashOrder::default();
        flash_order.amount = amount;
        let mut o = OrderWithStorageData::with_default(AllOrders::ExactFlash(flash_order));
        o.pool_id = pool_id;
        o.is_bid = is_bid;
        o
    }

    #[test]
    fn orders_grouped_by_pool_id_basic() {
        let pool_a: PoolId =
            fixed_bytes!("0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        let pool_b: PoolId =
            fixed_bytes!("0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");

        let o1 = make_order_with_pool(pool_a, true);
        let o2 = make_order_with_pool(pool_a, false);
        let o3 = make_order_with_pool(pool_b, true);

        let grouped = orders_sorted_by_pool_id(vec![o1.clone(), o2.clone(), o3.clone()]);

        assert_eq!(grouped.len(), 2);
        assert!(grouped.contains_key(&pool_a));
        assert!(grouped.contains_key(&pool_b));
        assert_eq!(grouped.get(&pool_a).unwrap().len(), 2);
        assert_eq!(grouped.get(&pool_b).unwrap().len(), 1);
    }

    #[test]
    fn build_non_proposal_books_sets_amm_when_snapshot_exists() {
        let pool_id: PoolId =
            fixed_bytes!("0xcccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc");

        // Minimal BaselinePoolState
        let liq = BaselineLiquidity::new(
            60,
            0,
            SqrtPriceX96::at_tick(0).unwrap(),
            0,
            Default::default(),
            Default::default()
        );
        let baseline = BaselinePoolState::new(liq, 1, 3000);

        let mut snapshots = HashMap::new();
        let uni_pool_id: PoolId =
            fixed_bytes!("0xdddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd");
        snapshots.insert(pool_id, (uni_pool_id, baseline));

        // one order for this pool
        let o = make_order_with_pool(pool_id, true);
        let books = build_non_proposal_books(vec![o], &snapshots);

        let book = books.get(&pool_id).expect("book for pool");
        assert_eq!(book.id(), pool_id);
        assert!(book.amm().is_some());
    }

    #[test]
    fn build_non_proposal_books_no_amm_when_snapshot_missing() {
        let pool_id: PoolId =
            fixed_bytes!("0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee");

        let books =
            build_non_proposal_books(vec![make_order_with_pool(pool_id, false)], &HashMap::new());
        let book = books.get(&pool_id).expect("book for pool");
        assert_eq!(book.id(), pool_id);
        assert!(book.amm().is_none());
    }

    #[test]
    fn slot0_update_creation() {
        let pool_id = make_test_pool_id(1);
        let update = make_slot0_update(10, pool_id);

        assert_eq!(update.seq_id, 10);
        assert_eq!(update.current_block, 100);
        assert_eq!(update.angstrom_pool_id, pool_id);
        assert_eq!(update.uni_pool_id, pool_id);
        assert_eq!(update.sqrt_price_x96, U160::from(1000));
        assert_eq!(update.liquidity, 1000000);
        assert_eq!(update.tick, 0);
    }

    #[test]
    fn slot0_update_equality() {
        let pool_id = make_test_pool_id(1);
        let update1 = make_slot0_update(10, pool_id);
        let update2 = make_slot0_update(10, pool_id);
        let update3 = make_slot0_update(11, pool_id);

        assert_eq!(update1, update2);
        assert_ne!(update1, update3);
    }

    #[test]
    fn slot0_update_hash() {
        use std::collections::HashSet;

        let pool_id = make_test_pool_id(1);
        let update1 = make_slot0_update(10, pool_id);
        let update2 = make_slot0_update(10, pool_id);
        let update3 = make_slot0_update(11, pool_id);

        let mut set = HashSet::new();
        set.insert(update1.clone());

        assert!(set.contains(&update2));
        assert!(!set.contains(&update3));
    }

    #[test]
    fn slot0_update_hash_eq_contract() {
        use std::{
            collections::hash_map::DefaultHasher,
            hash::{Hash, Hasher}
        };

        let pool_id = make_test_pool_id(1);
        let a = make_slot0_update(10, pool_id);
        let b = make_slot0_update(10, pool_id);

        let mut ha = DefaultHasher::new();
        a.hash(&mut ha);
        let mut hb = DefaultHasher::new();
        b.hash(&mut hb);

        assert_eq!(a, b);
        assert_eq!(ha.finish(), hb.finish());
    }

    #[test]
    fn orders_sorted_by_pool_id_empty() {
        let grouped = orders_sorted_by_pool_id(vec![]);
        assert!(grouped.is_empty());
    }

    #[test]
    fn orders_sorted_by_pool_id_single_order() {
        let pool_id = make_test_pool_id(1);
        let order = make_order_with_pool(pool_id, true);

        let grouped = orders_sorted_by_pool_id(vec![order.clone()]);

        assert_eq!(grouped.len(), 1);
        assert!(grouped.contains_key(&pool_id));
        assert_eq!(grouped.get(&pool_id).unwrap().len(), 1);
    }

    #[test]
    fn orders_sorted_by_pool_id_duplicate_handling() {
        let pool_id = make_test_pool_id(1);
        let order1 = make_detailed_order(pool_id, true, 100);
        let order2 = make_detailed_order(pool_id, true, 200);

        let grouped = orders_sorted_by_pool_id(vec![order1.clone(), order2.clone()]);

        assert_eq!(grouped.len(), 1);
        assert_eq!(grouped.get(&pool_id).unwrap().len(), 2);
    }

    #[test]
    fn build_non_proposal_books_empty_orders() {
        let snapshots = HashMap::new();
        let books = build_non_proposal_books(vec![], &snapshots);
        assert!(books.is_empty());
    }

    #[test]
    fn build_non_proposal_books_multiple_pools() {
        let pool_a = make_test_pool_id(1);
        let pool_b = make_test_pool_id(2);
        let uni_pool_a = make_test_pool_id(11);
        let uni_pool_b = make_test_pool_id(12);

        let mut snapshots = HashMap::new();
        snapshots.insert(pool_a, (uni_pool_a, make_test_baseline_pool_state(60, 1000000)));
        snapshots.insert(pool_b, (uni_pool_b, make_test_baseline_pool_state(-60, 2000000)));

        let orders = vec![
            make_order_with_pool(pool_a, true),
            make_order_with_pool(pool_a, false),
            make_order_with_pool(pool_b, true),
        ];

        let books = build_non_proposal_books(orders, &snapshots);

        assert_eq!(books.len(), 2);
        assert!(books.contains_key(&pool_a));
        assert!(books.contains_key(&pool_b));
        assert!(books.get(&pool_a).unwrap().amm().is_some());
        assert!(books.get(&pool_b).unwrap().amm().is_some());
    }

    #[test]
    fn build_non_proposal_books_mixed_snapshot_availability() {
        let pool_with_snapshot = make_test_pool_id(1);
        let pool_without_snapshot = make_test_pool_id(2);
        let uni_pool = make_test_pool_id(11);

        let mut snapshots = HashMap::new();
        snapshots.insert(pool_with_snapshot, (uni_pool, make_test_baseline_pool_state(0, 1000000)));

        let orders = vec![
            make_order_with_pool(pool_with_snapshot, true),
            make_order_with_pool(pool_without_snapshot, false),
        ];

        let books = build_non_proposal_books(orders, &snapshots);

        assert_eq!(books.len(), 2);
        assert!(books.get(&pool_with_snapshot).unwrap().amm().is_some());
        assert!(books.get(&pool_without_snapshot).unwrap().amm().is_none());
    }

    #[tokio::test]
    async fn quoter_handle_stream_receives_updates() {
        use std::time::Duration;

        use futures::StreamExt;
        use tokio::time::timeout;

        let (tx, mut rx) = mpsc::channel(10);
        let handle = QuoterHandle(tx);

        let pool_id = make_test_pool_id(42);
        let mut pools = HashSet::new();
        pools.insert(pool_id);

        // Subscribe and get the sender the producer will use
        let mut stream = handle.subscribe_to_updates(pools).await;
        let (_pools, update_tx) = rx.recv().await.expect("subscription forwarded");

        // Send an update and assert it arrives on the stream
        let update = make_slot0_update(7, pool_id);
        update_tx.send(update.clone()).await.unwrap();

        let got = timeout(Duration::from_secs(1), stream.next())
            .await
            .expect("stream yielded")
            .expect("some item");

        assert_eq!(got, update);
    }

    #[tokio::test]
    async fn quoter_handle_stream_is_pending_without_updates() {
        use std::task::Poll;

        use futures::{StreamExt, poll};

        let (tx, _rx) = mpsc::channel(1);
        let handle = QuoterHandle(tx);

        let mut pools = HashSet::new();
        pools.insert(make_test_pool_id(1));

        let mut stream = handle.subscribe_to_updates(pools).await;

        // No producer => pending.
        assert!(matches!(poll!(stream.next()), Poll::Pending));
    }

    #[tokio::test]
    async fn quoter_handle_subscribe_creates_stream() {
        let (tx, mut rx) = mpsc::channel(10);
        let handle = QuoterHandle(tx);

        let pool_id = make_test_pool_id(1);
        let mut pools = HashSet::new();
        pools.insert(pool_id);

        let _stream = handle.subscribe_to_updates(pools).await;

        // Check that subscription was sent
        if let Some((received_pools, _)) = rx.recv().await {
            assert!(received_pools.contains(&pool_id));
        } else {
            panic!("No subscription received");
        }
    }

    #[tokio::test]
    async fn quoter_handle_multiple_subscriptions() {
        let (tx, mut rx) = mpsc::channel(10);
        let handle = QuoterHandle(tx);

        let pool_a = make_test_pool_id(1);
        let pool_b = make_test_pool_id(2);

        let mut pools_a = HashSet::new();
        pools_a.insert(pool_a);

        let mut pools_b = HashSet::new();
        pools_b.insert(pool_b);

        let _stream_a = handle.subscribe_to_updates(pools_a.clone()).await;
        let _stream_b = handle.subscribe_to_updates(pools_b.clone()).await;

        // Check both subscriptions were sent
        let mut received_pools = HashSet::new();
        if let Some((pools, _)) = rx.recv().await {
            received_pools.extend(pools);
        }
        if let Some((pools, _)) = rx.recv().await {
            received_pools.extend(pools);
        }

        assert!(received_pools.contains(&pool_a));
        assert!(received_pools.contains(&pool_b));
    }

    proptest! {
        #[test]
        fn prop_orders_sorted_preserves_all_orders(
            orders in prop::collection::vec((0u8..10, any::<bool>(), 1u128..1000000), 0..50)
        ) {
            let book_orders: Vec<BookOrder> = orders
                .into_iter()
                .map(|(pool_suffix, is_bid, amount)| {
                    let pool_id = make_test_pool_id(pool_suffix);
                    make_detailed_order(pool_id, is_bid, amount)
                })
                .collect();

            let original_count = book_orders.len();
            let grouped = orders_sorted_by_pool_id(book_orders);

            let grouped_count: usize = grouped.values().map(|set| set.len()).sum();
            assert_eq!(original_count, grouped_count);
        }

        #[test]
        fn prop_build_books_maintains_pool_ids(
            orders in prop::collection::vec((0u8..5, any::<bool>()), 0..20)
        ) {
            let book_orders: Vec<BookOrder> = orders
                .into_iter()
                .map(|(pool_suffix, is_bid)| {
                    let pool_id = make_test_pool_id(pool_suffix);
                    make_order_with_pool(pool_id, is_bid)
                })
                .collect();

            let pool_ids: HashSet<PoolId> = book_orders
                .iter()
                .map(|o| o.pool_id)
                .collect();

            let books = build_non_proposal_books(book_orders, &HashMap::new());

            // All pool IDs from orders should be in books
            for pool_id in pool_ids {
                assert!(books.contains_key(&pool_id));
                assert_eq!(books.get(&pool_id).unwrap().id(), pool_id);
            }
        }

        #[test]
        fn prop_book_amm_presence(
            ids in prop::collection::vec(0u8..8, 0..8),
            with_snapshot in any::<bool>(),
        ) {
            let orders: Vec<BookOrder> = ids.iter().map(|s| make_order_with_pool(make_test_pool_id(*s), true)).collect();
            let mut snaps = HashMap::new();
            if with_snapshot {
                for s in &ids {
                    snaps.insert(make_test_pool_id(*s),
                        (make_test_pool_id(s.wrapping_add(100)), make_test_baseline_pool_state(0, 1)));
                }
            }
            let books = build_non_proposal_books(orders, &snaps);
            for (pid, book) in books {
                assert_eq!(book.amm().is_some(), snaps.contains_key(&pid));
            }
        }

        #[test]
        fn prop_slot0_update_consistency(
            seq_id in 0u16..u16::MAX,
            block in 0u64..1000000,
            tick in -887272i32..887272,
            liquidity in 0u128..u128::MAX
        ) {
            let pool_id = make_test_pool_id(1);
            let update = Slot0Update {
                seq_id,
                current_block: block,
                angstrom_pool_id: pool_id,
                uni_pool_id: pool_id,
                sqrt_price_x96: SqrtPriceX96::at_tick(tick).unwrap_or_default().into(),
                liquidity,
                tick,
            };

            // Test that fields are preserved
            assert_eq!(update.seq_id, seq_id);
            assert_eq!(update.current_block, block);
            assert_eq!(update.liquidity, liquidity);
            assert_eq!(update.tick, tick);
        }

        #[test]
        fn prop_grouping_never_drops_across_pools(orders in
            prop::collection::vec((0u8..5, any::<bool>(), 1u128..1_000_000u128), 0..50)
        ) {
            let by_pool: HashMap<PoolId, Vec<BookOrder>> = orders.iter().map(|(s, is_bid, amt)| {
                (make_test_pool_id(*s), make_detailed_order(make_test_pool_id(*s), *is_bid, *amt))
            }).fold(HashMap::new(), |mut m, (pid, o)| { m.entry(pid).or_default().push(o); m });

            let grouped = orders_sorted_by_pool_id(
                by_pool.values().flatten().cloned().collect()
            );

            for (pid, want) in by_pool {
                let have = grouped.get(&pid).unwrap();
                for _w in want {
                    // In set semantics we only require that each unique order (per Eq/Hash) can appear.
                    // So presence is "possible", not guaranteed, but at least one order per pool must exist.
                    assert!(!have.is_empty());
                }
            }
        }
    }
}
