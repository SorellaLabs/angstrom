use alloy::{
    primitives::{Address, FixedBytes, U256},
    sol_types::Eip712Domain
};
use angstrom_types::{
    orders::{OrderId, OrderPriorityData},
    primitive::PoolId,
    sol_bindings::{
        ext::RawPoolOrder,
        grouped_orders::{GroupedVanillaOrder, OrderWithStorageData},
        rpc_orders::TopOfBlockOrder
    }
};
use enr::k256::ecdsa::SigningKey;
use rand::{rngs::ThreadRng, Rng};

// mod stored;
mod distribution;
mod tob;
mod user;
pub use distribution::OrderDistributionBuilder;
pub use tob::ToBOrderBuilder;
pub use user::UserOrderBuilder;

// fn build_priority_data(order: &GroupedVanillaOrder) -> OrderPriorityData {
//     OrderPriorityData { price: order.price().into(), volume: order.quantity()
// as u128, gas: 10 } }

#[derive(Clone, Debug)]
pub struct SigningInfo {
    pub domain:  Eip712Domain,
    pub address: Address,
    pub key:     SigningKey
}

#[derive(Clone, Debug)]
pub struct StoredOrderBuilder {
    order:       GroupedVanillaOrder,
    is_bid:      bool,
    pool_id:     Option<FixedBytes<32>>,
    valid_block: Option<u64>,
    tob_reward:  Option<U256>
}

impl StoredOrderBuilder {
    pub fn new(order: GroupedVanillaOrder) -> Self {
        Self { order, is_bid: false, pool_id: None, valid_block: None, tob_reward: None }
    }

    pub fn from_builder(user_order: UserOrderBuilder) -> Self {
        Self::new(user_order.build())
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

    pub fn build(self) -> OrderWithStorageData<GroupedVanillaOrder> {
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
            gas:    U256::ZERO
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

#[derive(Clone, Debug, Default)]
pub struct OrderIdBuilder {
    address:    Option<Address>,
    pool_id:    Option<PoolId>,
    order_hash: Option<FixedBytes<32>>
}

impl OrderIdBuilder {
    pub fn new() -> Self {
        Self { ..Default::default() }
    }

    pub fn address(self, address: Address) -> Self {
        Self { address: Some(address), ..self }
    }

    pub fn pool_id(self, pool_id: PoolId) -> Self {
        Self { pool_id: Some(pool_id), ..self }
    }

    pub fn order_hash(self, order_hash: FixedBytes<32>) -> Self {
        Self { order_hash: Some(order_hash), ..self }
    }

    pub fn build(self) -> OrderId {
        let address = self.address.unwrap_or_default();
        let pool_id = self.pool_id.unwrap_or_default();
        let hash = self.order_hash.unwrap_or_default();
        OrderId {
            address,
            pool_id,
            hash,
            flash_block: None,
            location: Default::default(),
            deadline: None,
            reuse_avoidance: angstrom_types::sol_bindings::RespendAvoidanceMethod::Block(0)
        }
    }
}

pub fn generate_top_of_block_order(
    rng: &mut ThreadRng,
    is_bid: bool,
    pool_id: Option<PoolId>,
    valid_block: Option<u64>,
    quantity_in: Option<u128>,
    quantity_out: Option<u128>
) -> OrderWithStorageData<TopOfBlockOrder> {
    let pool_id = pool_id.unwrap_or_default();
    let valid_block = valid_block.unwrap_or_default();
    // Could update this to be within a distribution
    let price: u128 = rng.gen();
    let volume: u128 = rng.gen();
    let gas: U256 = rng.gen();
    let order = ToBOrderBuilder::new()
        .quantity_in(quantity_in.unwrap_or_default())
        .quantity_out(quantity_out.unwrap_or_default())
        .build();

    let priority_data = OrderPriorityData { price: U256::from(price), volume, gas };
    let order_id = OrderIdBuilder::new()
        .pool_id(pool_id)
        .order_hash(order.order_hash())
        .build();
    // Todo: Sign It, make this overall better
    // StoredOrderBuilder::new(order).is_bid(is_bid).valid_block(valid_block).
    // pool_id(pool_id).build();
    OrderWithStorageData {
        invalidates: vec![],
        order,
        priority_data,
        is_bid,
        is_currently_valid: true,
        is_valid: true,
        order_id,
        pool_id,
        valid_block,
        tob_reward: U256::ZERO
    }
}

#[derive(Debug, Default)]
pub struct DistributionParameters {
    pub location: f64,
    pub scale:    f64,
    pub shape:    f64
}

impl DistributionParameters {
    pub fn crossed_at(location: f64) -> (Self, Self) {
        let bids = Self { location, scale: 100000.0, shape: -2.0 };
        let asks = Self { location, scale: 100000.0, shape: 2.0 };

        (bids, asks)
    }

    pub fn fixed_at(location: f64) -> (Self, Self) {
        let bids = Self { location, scale: 1.0, shape: 0.0 };
        let asks = Self { location, scale: 1.0, shape: 0.0 };

        (bids, asks)
    }
}
