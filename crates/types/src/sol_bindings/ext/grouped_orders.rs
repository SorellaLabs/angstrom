use std::{fmt, ops::Deref};

use alloy_primitives::{Address, FixedBytes, TxHash, U256};
use alloy_sol_types::SolStruct;
use bincode::{Decode, Encode};
use reth_primitives::B256;
use serde::{Deserialize, Serialize};

use super::FetchAssetIndexes;
use crate::{
    orders::{OrderId, OrderPriorityData},
    primitive::PoolId,
    sol_bindings::sol::{FlashOrder, StandingOrder, TopOfBlockOrder}
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash, Encode, Decode)]
pub enum AllOrders {
    Partial(#[bincode(with_serde)] StandingOrder),
    KillOrFill(#[bincode(with_serde)] FlashOrder),
    TOB(#[bincode(with_serde)] TopOfBlockOrder)
}

impl AllOrders {}

impl From<TopOfBlockOrder> for AllOrders {
    fn from(value: TopOfBlockOrder) -> Self {
        Self::TOB(value)
    }
}
impl From<GroupedComposableOrder> for AllOrders {
    fn from(value: GroupedComposableOrder) -> Self {
        match value {
            GroupedComposableOrder::Partial(p) => AllOrders::Partial(p),
            GroupedComposableOrder::KillOrFill(kof) => AllOrders::KillOrFill(kof)
        }
    }
}

impl From<GroupedVanillaOrder> for AllOrders {
    fn from(value: GroupedVanillaOrder) -> Self {
        match value {
            GroupedVanillaOrder::Partial(p) => AllOrders::Partial(p),
            GroupedVanillaOrder::KillOrFill(kof) => AllOrders::KillOrFill(kof)
        }
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

#[derive(Clone, Debug)]
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

#[derive(Debug, Clone)]
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

pub trait PoolOrder: fmt::Debug + Send + Sync + Clone + Unpin + 'static {
    /// Hash of the order
    fn hash(&self) -> TxHash;

    /// The order signer
    fn from(&self) -> Address;

    /// Transaction nonce
    fn nonce(&self) -> U256;

    /// Amount of tokens to sell
    fn amount_in(&self) -> u128;

    /// Token in
    fn token_in(&self) -> Address;

    /// Min amount of tokens to buy
    fn amount_out_min(&self) -> u128;

    /// Token out
    fn token_out(&self) -> Address;

    /// Limit Price
    fn limit_price(&self) -> u128;

    /// Order deadline
    fn deadline(&self) -> U256;
}

/// The capability of all default orders.
pub trait RawPoolOrder:
    FetchAssetIndexes + fmt::Debug + Send + Sync + Clone + Unpin + 'static
{
    /// Hash of the order
    fn hash(&self) -> TxHash;

    /// The order signer
    fn from(&self) -> Address;

    /// Transaction nonce
    fn nonce(&self) -> U256;

    /// Amount of tokens to sell
    fn amount_in(&self) -> u128;

    /// Min amount of tokens to buy
    fn amount_out_min(&self) -> u128;

    /// Limit Price
    fn limit_price(&self) -> u128;

    /// Order deadline
    fn deadline(&self) -> U256;
}

impl FetchAssetIndexes for TopOfBlockOrder {
    fn get_token_in(&self) -> u16 {
        self.assetInIndex
    }

    fn get_token_out(&self) -> u16 {
        self.assetOutIndex
    }
}

impl RawPoolOrder for TopOfBlockOrder {
    fn from(&self) -> Address {
        self.from
    }

    fn hash(&self) -> TxHash {
        self.eip712_hash_struct()
    }

    fn nonce(&self) -> U256 {
        U256::default()
    }

    fn deadline(&self) -> U256 {
        U256::default()
    }

    fn amount_in(&self) -> u128 {
        self.amountIn.to()
    }

    fn limit_price(&self) -> u128 {
        self.amount_in() / self.amount_out_min()
    }

    fn amount_out_min(&self) -> u128 {
        self.amountOut.to()
    }
}

impl RawPoolOrder for GroupedVanillaOrder {
    fn from(&self) -> Address {
        match self {
            GroupedVanillaOrder::Partial(p) => p.recipient,
            GroupedVanillaOrder::KillOrFill(kof) => kof.recipient
        }
    }

    fn hash(&self) -> TxHash {
        match self {
            GroupedVanillaOrder::Partial(p) => p.eip712_hash_struct(),
            GroupedVanillaOrder::KillOrFill(kof) => kof.eip712_hash_struct()
        }
    }

    fn nonce(&self) -> U256 {
        match self {
            GroupedVanillaOrder::Partial(p) => U256::from(p.nonce),
            GroupedVanillaOrder::KillOrFill(_) => U256::ZERO
        }
    }

    fn deadline(&self) -> U256 {
        match self {
            GroupedVanillaOrder::Partial(p) => p.deadline,
            GroupedVanillaOrder::KillOrFill(_) => U256::ZERO
        }
    }

    fn amount_in(&self) -> u128 {
        match self {
            GroupedVanillaOrder::Partial(p) => p.max_amount_in_or_out.to(),
            GroupedVanillaOrder::KillOrFill(kof) => kof.max_amount_in_or_out.to()
        }
    }

    fn limit_price(&self) -> u128 {
        match self {
            GroupedVanillaOrder::Partial(p) => p.min_price.to(),
            GroupedVanillaOrder::KillOrFill(p) => p.min_price.to()
        }
    }

    fn amount_out_min(&self) -> u128 {
        match self {
            GroupedVanillaOrder::Partial(p) => p.max_amount_in_or_out.to(),
            GroupedVanillaOrder::KillOrFill(kof) => kof.max_amount_in_or_out.to()
        }
    }
}
impl FetchAssetIndexes for GroupedVanillaOrder {
    fn get_token_in(&self) -> u16 {
        match self {
            GroupedVanillaOrder::Partial(p) => p.asset_in,
            GroupedVanillaOrder::KillOrFill(kof) => kof.asset_in
        }
    }

    fn get_token_out(&self) -> u16 {
        match self {
            GroupedVanillaOrder::Partial(p) => p.asset_out,
            GroupedVanillaOrder::KillOrFill(kof) => kof.asset_out
        }
    }
}

impl FetchAssetIndexes for AllOrders {
    fn get_token_in(&self) -> u16 {
        match self {
            AllOrders::Partial(p) => p.asset_in,
            AllOrders::KillOrFill(kof) => kof.asset_in,
            AllOrders::TOB(tob) => tob.assetInIndex
        }
    }

    fn get_token_out(&self) -> u16 {
        match self {
            AllOrders::Partial(p) => p.asset_out,
            AllOrders::KillOrFill(kof) => kof.asset_out,
            AllOrders::TOB(tob) => tob.assetOutIndex
        }
    }
}

impl RawPoolOrder for AllOrders {
    fn from(&self) -> Address {
        match self {
            AllOrders::Partial(p) => p.recipient,
            AllOrders::KillOrFill(kof) => kof.recipient,
            AllOrders::TOB(tob) => tob.from
        }
    }

    fn hash(&self) -> TxHash {
        match self {
            AllOrders::Partial(p) => p.eip712_hash_struct(),
            AllOrders::KillOrFill(kof) => kof.eip712_hash_struct(),
            AllOrders::TOB(tob) => tob.eip712_hash_struct()
        }
    }

    fn nonce(&self) -> U256 {
        match self {
            AllOrders::Partial(p) => U256::from(p.nonce),
            AllOrders::KillOrFill(_) => U256::ZERO,
            AllOrders::TOB(_) => U256::ZERO
        }
    }

    fn deadline(&self) -> U256 {
        match self {
            AllOrders::Partial(p) => p.deadline,
            AllOrders::KillOrFill(_) => U256::ZERO,
            AllOrders::TOB(_) => U256::ZERO
        }
    }

    fn amount_in(&self) -> u128 {
        match self {
            AllOrders::Partial(p) => p.max_amount_in_or_out.to(),
            AllOrders::KillOrFill(kof) => kof.max_amount_in_or_out.to(),
            AllOrders::TOB(tob) => tob.amountIn.to()
        }
    }

    fn limit_price(&self) -> u128 {
        match self {
            AllOrders::Partial(p) => p.min_price.to(),
            AllOrders::KillOrFill(kof) => kof.min_price.to(),
            AllOrders::TOB(_) => self.amount_in() / self.amount_out_min()
        }
    }

    fn amount_out_min(&self) -> u128 {
        match self {
            AllOrders::Partial(p) => p.max_amount_in_or_out.to(),
            AllOrders::KillOrFill(kof) => kof.max_amount_in_or_out.to(),
            AllOrders::TOB(tob) => { tob.amountOut }.to()
        }
    }
}

impl FetchAssetIndexes for GroupedComposableOrder {
    fn get_token_in(&self) -> u16 {
        match self {
            GroupedComposableOrder::Partial(p) => p.asset_in,
            GroupedComposableOrder::KillOrFill(kof) => kof.asset_in
        }
    }

    fn get_token_out(&self) -> u16 {
        match self {
            GroupedComposableOrder::Partial(p) => p.asset_out,
            GroupedComposableOrder::KillOrFill(kof) => kof.asset_out
        }
    }
}

impl RawPoolOrder for GroupedComposableOrder {
    fn from(&self) -> Address {
        match self {
            GroupedComposableOrder::Partial(p) => p.recipient,
            GroupedComposableOrder::KillOrFill(kof) => kof.recipient
        }
    }

    fn hash(&self) -> TxHash {
        match self {
            GroupedComposableOrder::Partial(p) => p.eip712_hash_struct(),
            GroupedComposableOrder::KillOrFill(kof) => kof.eip712_hash_struct()
        }
    }

    fn nonce(&self) -> U256 {
        match self {
            GroupedComposableOrder::Partial(p) => U256::from(p.nonce),
            GroupedComposableOrder::KillOrFill(_) => U256::ZERO
        }
    }

    fn deadline(&self) -> U256 {
        match self {
            GroupedComposableOrder::Partial(p) => p.deadline,
            GroupedComposableOrder::KillOrFill(_) => U256::ZERO
        }
    }

    fn amount_in(&self) -> u128 {
        match self {
            GroupedComposableOrder::Partial(p) => p.max_amount_in_or_out.to(),
            GroupedComposableOrder::KillOrFill(kof) => kof.max_amount_in_or_out.to()
        }
    }

    fn limit_price(&self) -> u128 {
        match self {
            GroupedComposableOrder::Partial(p) => p.min_price.to(),
            GroupedComposableOrder::KillOrFill(p) => p.min_price.to()
        }
    }

    fn amount_out_min(&self) -> u128 {
        match self {
            GroupedComposableOrder::Partial(p) => p.max_amount_in_or_out.to(),
            GroupedComposableOrder::KillOrFill(kof) => kof.max_amount_in_or_out.to()
        }
    }
}
