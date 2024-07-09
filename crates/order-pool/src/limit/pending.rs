use std::{cmp::Reverse, collections::BTreeMap};

use angstrom_types::orders::OrderPriorityData;
use revm::primitives::HashMap;
use sol_bindings::{ext::grouped_orders::GroupedVanillaOrders, grouped_orders::OrderWithId};

pub struct PendingPool {
    /// all order hashes
    orders: HashMap<u128, GroupedVanillaOrders>,
    /// bids are sorted descending by price, TODO: This should be binned into
    /// ticks based off of the underlying pools params
    bids:   BTreeMap<Reverse<OrderPriorityData>, u128>,
    /// asks are sorted ascending by price,  TODO: This should be binned into
    /// ticks based off of the underlying pools params
    asks:   BTreeMap<OrderPriorityData, u128>
}

impl PendingPool {
    #[allow(unused)]
    pub fn new() -> Self {
        Self { orders: HashMap::new(), bids: BTreeMap::new(), asks: BTreeMap::new() }
    }

    pub fn add_order(&mut self, order: OrderWithId<GroupedVanillaOrders>) {
        // let hash = order.hash();
        // let priority = order.priority_data();
        //
        // if order.is_bid() {
        //     self.bids.insert(Reverse(priority), hash);
        // } else {
        //     self.asks.insert(priority, hash);
        // }
        //
        // self.orders.insert(hash, order.clone());
    }

    pub fn remove_order(&mut self, hash: u128) -> Option<GroupedVanillaOrders> {
        // let order = self.orders.remove(&hash)?;
        // let priority = order.priority_data();
        //
        // if order.is_bid() {
        //     self.bids.remove(&Reverse(priority))?;
        // } else {
        //     self.asks.remove(&priority)?;
        // }
        //
        // Some(order)
        None
    }
}

#[cfg(test)]
pub mod test {

    // #[derive(Debug, Clone, Copy)]
    // pub struct NoopOrder {
    //     price:    u128,
    //     volume:   u128,
    //     gas:      u128,
    //     hash:     B256,
    //     is_bid:   bool,
    //     nonce:    u64,
    //     deadline: u128
    // }
    //
    // impl PoolOrder for NoopOrder {
    //     type ValidationData = ();
    //
    //     fn token_in(&self) -> Address {
    //         todo!()
    //     }
    //
    //     fn token_out(&self) -> Address {
    //         todo!()
    //     }
    //
    //     fn is_bid(&self) -> bool {
    //         self.is_bid
    //     }
    //
    //     fn is_valid(&self) -> bool {
    //         true
    //     }
    //
    //     fn order_id(&self) -> OrderId {
    //         OrderId {
    //             address:  Address::ZERO,
    //             pool_id:  Address::ZERO,
    //             hash:     self.hash,
    //             nonce:    self.nonce,
    //             deadline: self.deadline
    //         }
    //     }
    //
    //     fn hash(&self) -> alloy_primitives::TxHash {
    //         self.hash
    //     }
    //
    //     fn from(&self) -> Address {
    //         Address::ZERO
    //     }
    //
    //     fn size(&self) -> usize {
    //         69
    //     }
    //
    //     fn nonce(&self) -> alloy_primitives::U256 {
    //         U256::from(self.nonce)
    //     }
    //
    //     fn deadline(&self) -> alloy_primitives::U256 {
    //         U256::from(self.deadline)
    //     }
    //
    //     fn chain_id(&self) -> Option<u64> {
    //         None
    //     }
    //
    //     fn amount_in(&self) -> u128 {
    //         0
    //     }
    //
    //     fn limit_price(&self) -> u128 {
    //         self.price
    //     }
    //
    //     fn amount_out_min(&self) -> u128 {
    //         0
    //     }
    //
    //     fn encoded_length(&self) -> usize {
    //         4123
    //     }
    // }
    //
    // pub fn generate_noop_orders(bids: usize, asks: usize) ->
    // Vec<ValidatedOrder<NoopOrder, ()>> {     let mut res =
    // Vec::with_capacity(bids + asks);     let mut rng =
    // rand::thread_rng();     for _ in 0..bids {
    //         res.push(ValidatedOrder {
    //             order: NoopOrder {
    //                 price:    rng.gen(),
    //                 volume:   rng.gen(),
    //                 deadline: rng.gen(),
    //                 nonce:    rng.gen(),
    //                 hash:     rng.gen(),
    //                 is_bid:   true,
    //                 gas:      rng.gen()
    //             },
    //             data:  ()
    //         })
    //     }
    //
    //     for _ in 0..asks {
    //         res.push(ValidatedOrder {
    //             order: NoopOrder {
    //                 price:    rng.gen(),
    //                 volume:   rng.gen(),
    //                 deadline: rng.gen(),
    //                 nonce:    rng.gen(),
    //                 hash:     rng.gen(),
    //                 is_bid:   false,
    //                 gas:      rng.gen()
    //             },
    //             data:  ()
    //         })
    //     }
    //
    //     res
    // }
    //
    // pub fn init_pool_with_data(
    //     sender: broadcast::Sender<ValidatedOrder<NoopOrder, ()>>,
    //     bids: usize,
    //     asks: usize
    // ) -> PendingPool<NoopOrder> {
    //     let mut pool = PendingPool::new(sender);
    //     let orders = generate_noop_orders(bids, asks);
    //     orders.into_iter().for_each(|order| pool.new_order(order));
    //
    //     pool
    // }
    //
    // macro_rules! order_priority {
    //     ($price:expr, $vol:expr, $gas:expr) => {
    //         OrderPriorityData { price: $price, volume: $vol, gas: $gas }
    //     };
    // }
    //
    // #[test]
    // pub fn verify_order_priority_cmp() {
    //     let p0 = order_priority!(101, 1000, 10);
    //     let p1 = order_priority!(100, 1001, 10);
    //     let p2 = order_priority!(99, 1001, 10);
    //
    //     let p3 = order_priority!(100, 1001, 10);
    //     let p4 = order_priority!(100, 1000, 10);
    //     let p5 = order_priority!(100, 999, 10);
    //
    //     let p6 = order_priority!(100, 1000, 11);
    //     let p7 = order_priority!(100, 1000, 10);
    //     let p8 = order_priority!(100, 1000, 9);
    //
    //     let gt = Ordering::Greater;
    //     let lt = Ordering::Less;
    //     let eq = Ordering::Equal;
    //
    //     assert_eq!(p1.cmp(&p0), lt);
    //     assert_eq!(p1.cmp(&p1), eq);
    //     assert_eq!(p1.cmp(&p2), gt);
    //
    //     assert_eq!(p4.cmp(&p3), lt);
    //     assert_eq!(p4.cmp(&p4), eq);
    //     assert_eq!(p4.cmp(&p5), gt);
    //
    //     assert_eq!(p7.cmp(&p6), lt);
    //     assert_eq!(p7.cmp(&p7), eq);
    //     assert_eq!(p7.cmp(&p8), gt);
    // }
    //
    // macro_rules! is_ordered {
    //     ($list:expr, $cmp:tt, $field:ident) => {
    //         let mut last = $list.remove(0);
    //         for entry in $list {
    //             assert!(entry.$field $cmp last.$field);
    //             last = entry ;
    //         }
    //     };
    // }
    //
    // #[test]
    // pub fn verify_order_insertion() {
    //     let (tx, mut rx) = broadcast::channel(15);
    //     let pool = init_pool_with_data(tx, 5, 5);
    //
    //     let mut count = 0;
    //     while let Ok(_) = rx.try_recv() {
    //         count += 1;
    //     }
    //     // verify all orders where sent via channel
    //     assert_eq!(count, 10);
    //
    //     // assert price order
    //     let mut asks = pool.asks.keys().collect::<Vec<_>>();
    //     is_ordered!(asks, >=, price);
    //
    //     // assert price order
    //     let mut bids = pool.bids.keys().map(|k| k.0).collect::<Vec<_>>();
    //     is_ordered!(bids, <=, price);
    // }
    //
    // #[test]
    // pub fn test_remove_bid() {
    //     let (tx, mut rx) = broadcast::channel(1500);
    //     let mut pool = init_pool_with_data(tx, 500, 500);
    //
    //     let mut count = 0;
    //     while let Ok(_) = rx.try_recv() {
    //         count += 1;
    //     }
    //
    //     let mut bids = pool.fetch_all_bids_meta();
    //     let (_, hash) = bids.remove(0);
    //
    //     let order = pool.remove_order(hash);
    //
    //     assert!(order.is_some());
    //
    //     assert!(pool
    //         .fetch_all_bids_meta()
    //         .into_iter()
    //         .find(|(_, has_hash)| has_hash == &hash)
    //         .is_none());
    //
    //     // verify all orders where sent via channel
    //     assert_eq!(count, 1000);
    // }
    //
    // #[test]
    // pub fn test_remove_ask() {
    //     let (tx, mut rx) = broadcast::channel(1500);
    //     let mut pool = init_pool_with_data(tx, 500, 500);
    //
    //     let mut count = 0;
    //     while let Ok(_) = rx.try_recv() {
    //         count += 1;
    //     }
    //
    //     // verify all orders where sent via channel
    //     assert_eq!(count, 1000);
    //
    //     let mut asks = pool.fetch_all_asks_meta();
    //     let (_, hash) = asks.remove(0);
    //
    //     let order = pool.remove_order(hash);
    //
    //     assert!(order.is_some());
    //
    //     assert!(pool
    //         .fetch_all_asks_meta()
    //         .into_iter()
    //         .find(|(_, has_hash)| has_hash == &hash)
    //         .is_none());
    // }
}
