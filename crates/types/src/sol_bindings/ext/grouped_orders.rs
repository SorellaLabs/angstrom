use std::ops::Deref;

use alloy_primitives::FixedBytes;
use alloy_sol_types::SolStruct;
use reth_primitives::B256;

use crate::{
    orders::OrderPriorityData,
    primitive::PoolId,
    sol_bindings::sol::{FlashOrder, StandingOrder, TopOfBlockOrder}
};

#[derive(Debug)]
pub enum AllOrders {
    Partial(StandingOrder),
    KillOrFill(FlashOrder),
    TOB(TopOfBlockOrder)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderWithStorageData<Order> {
    /// raw order
    pub order:              Order,
    /// internal order id
    pub id:                 u64,
    /// the raw data needed for indexing the data
    pub priority_data:      OrderPriorityData,
    /// the pool this order belongs to
    pub pool_id:            PoolId,
    /// wether the order is waiting for approvals / proper balances
    pub is_currently_valid: bool,
    /// what side of the book does this order lay on
    pub is_bid:             bool,
    /// is valid order
    pub is_valid:           bool
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
            id:                 self.id,
            is_bid:             self.is_bid,
            priority_data:      self.priority_data,
            is_currently_valid: self.is_currently_valid,
            is_valid:           self.is_valid
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
