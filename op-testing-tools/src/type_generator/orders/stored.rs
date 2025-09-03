use alloy::primitives::{FixedBytes, U256};
use angstrom_types::{
    orders::OrderPriorityData,
    sol_bindings::{grouped_orders::OrderWithStorageData, RawPoolOrder}
};

use super::{OrderIdBuilder, UserOrderBuilder};

#[derive(Clone, Debug)]
pub struct StoredOrderBuilder<Order: RawPoolOrder> {
    order:       Order,
    is_bid:      bool,
    pool_id:     Option<FixedBytes<32>>,
    valid_block: Option<u64>,
    tob_reward:  Option<U256>
}

impl<Order: RawPoolOrder> StoredOrderBuilder<Order> {
    pub fn new(order: Order) -> Self {
        Self { order, is_bid: false, pool_id: None, valid_block: None, tob_reward: None }
    }

    pub fn bid(self) -> Self {
        Self { is_bid: true, ..self }
    }

    pub fn ask(self) -> Self {
        Self { is_bid: false, ..self }
    }

    pub fn is_bid(self, is_bid: bool) -> Self {
        // Can this be determined from the order and the pool?  Maybe...
        Self { is_bid, ..self }
    }

    pub fn pool_id(self, pool_id: FixedBytes<32>) -> Self {
        Self { pool_id: Some(pool_id), ..self }
    }

    pub fn valid_block(self, valid_block: u64) -> Self {
        Self { valid_block: Some(valid_block), ..self }
    }

    pub fn tob_reward(self, tob_reward: U256) -> Self {
        Self { tob_reward: Some(tob_reward), ..self }
    }

    pub fn build(self) -> OrderWithStorageData<Order> {
        let is_bid = self.is_bid;
        let pool_id = self.pool_id.unwrap_or_default();
        let order_id = OrderIdBuilder::new()
            .pool_id(pool_id)
            .order_hash(self.order.hash())
            .build();
        // Our specified block or the order's specified block or default
        let valid_block = self
            .valid_block
            .or(self.order.flash_block())
            .unwrap_or_default();
        let priority_data = OrderPriorityData {
            price:  self.order.price().into(),
            volume: self.order.quantity().to(),
            gas:    0
        };
        let tob_reward = self.tob_reward.unwrap_or_default();
        OrderWithStorageData {
            invalidates: vec![],
            order: self.order,
            priority_data,
            is_bid,
            is_currently_valid: true,
            is_valid: true,
            order_id,
            pool_id,
            valid_block,
            tob_reward
        }
    }
}
