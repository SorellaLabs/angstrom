use std::ops::Deref;

use alloy_primitives::{Address, FixedBytes};
use alloy_sol_types::SolStruct;
use reth_primitives::B256;

use crate::{
    orders::{OrderId, OrderPriorityData},
    primitive::PoolId,
    sol_bindings::sol::{FlashOrder, StandingOrder, TopOfBlockOrder}
};

#[derive(Debug)]
pub enum AllOrders {
    Partial(StandingOrder),
    KillOrFill(FlashOrder),
    TOB(TopOfBlockOrder)
}

impl From<TopOfBlockOrder> for AllOrders {
    fn from(value: TopOfBlockOrder) -> Self {
        Self::TOB(value)
    }
}
impl From<GroupedUserOrder> for AllOrders {
    fn from(value: GroupedUserOrder) -> Self {
        match value {
            GroupedUserOrder::Vanilla(v) => match v {
                GroupedVanillaOrder::Partial(p) => AllOrders::Partial(p),
                GroupedVanillaOrder::KillOrFill(kof) => AllOrders::KillOrFill(kof)
            },
            GroupedUserOrder::Composable(v) => match v {
                GroupedComposableOrder::Partial(p) => AllOrders::Partial(p),
                GroupedComposableOrder::KillOrFill(kof) => AllOrders::KillOrFill(kof)
            }
        }
    }
}

impl AllOrders {
    pub fn order_hash(&self) -> FixedBytes<32> {
        match self {
            Self::Partial(p) => p.eip712_hash_struct(),
            Self::KillOrFill(f) => f.eip712_hash_struct(),
            Self::TOB(t) => t.eip712_hash_struct()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderWithStorageData<Order> {
    /// raw order
    pub order:              Order,
    /// the raw data needed for indexing the data
    pub priority_data:      OrderPriorityData,
    /// the pool this order belongs to
    pub pool_id:            PoolId,
    /// wether the order is waiting for approvals / proper balances
    pub is_currently_valid: bool,
    /// what side of the book does this order lay on
    pub is_bid:             bool,
    /// is valid order
    pub is_valid:           bool,
    /// the block the order was validated for
    pub valid_block:        u64,
    /// holds expiry data
    pub order_id:           OrderId
}

impl OrderWithStorageData<AllOrders> {
    pub fn from(&self) -> Address {
        match &self.order {
            AllOrders::KillOrFill(kof) => kof.recipient,
            AllOrders::Partial(p) => p.recipient,
            AllOrders::TOB(tob) => tob.recipient
        }
    }
}

impl<Order> Deref for OrderWithStorageData<Order> {
    type Target = Order;

    fn deref(&self) -> &Self::Target {
        &self.order
    }
}

impl<Order> OrderWithStorageData<Order> {
    pub fn size(&self) -> usize {
        std::mem::size_of::<Order>()
    }

    pub fn try_map_inner<NewOrder>(
        self,
        mut f: impl FnMut(Order) -> eyre::Result<NewOrder>
    ) -> eyre::Result<OrderWithStorageData<NewOrder>> {
        let new_order = f(self.order)?;

        Ok(OrderWithStorageData {
            order:              new_order,
            pool_id:            self.pool_id,
            valid_block:        self.valid_block,
            is_bid:             self.is_bid,
            priority_data:      self.priority_data,
            is_currently_valid: self.is_currently_valid,
            is_valid:           self.is_valid,
            order_id:           self.order_id
        })
    }
}

#[derive(Debug)]
pub enum GroupedUserOrder {
    Vanilla(GroupedVanillaOrder),
    Composable(GroupedComposableOrder)
}

impl GroupedUserOrder {
    pub fn is_vanilla(&self) -> bool {
        matches!(self, Self::Vanilla(_))
    }

    pub fn order_hash(&self) -> B256 {
        match self {
            GroupedUserOrder::Vanilla(v) => v.hash(),
            GroupedUserOrder::Composable(c) => c.hash()
        }
    }
}

#[derive(Debug)]
pub enum GroupedVanillaOrder {
    Partial(StandingOrder),
    KillOrFill(FlashOrder)
}

impl GroupedVanillaOrder {
    pub fn hash(&self) -> FixedBytes<32> {
        match self {
            Self::Partial(p) => p.eip712_hash_struct(),
            Self::KillOrFill(k) => k.eip712_hash_struct()
        }
    }
}

#[derive(Debug)]
pub enum GroupedComposableOrder {
    Partial(StandingOrder),
    KillOrFill(FlashOrder)
}

impl GroupedComposableOrder {
    pub fn hash(&self) -> B256 {
        match self {
            Self::Partial(p) => p.eip712_hash_struct(),
            Self::KillOrFill(k) => k.eip712_hash_struct()
        }
    }
}
