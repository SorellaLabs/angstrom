use std::fmt::Debug;

use alloy_primitives::{Address, Bytes, TxHash, U256};

use super::{
    super::{PoolOrder, PooledComposableOrder},
    ValidatedOrder
};
use crate::{
    orders::OrderConversion,
    primitive::PoolId,
    rpc::{
        EcRecoveredComposableLimitOrder, EcRecoveredLimitOrder, SignedComposableLimitOrder,
        SignedLimitOrder
    }
};

impl OrderConversion for EcRecoveredLimitOrder {
    type Order = SignedLimitOrder;

    fn try_from_order(order: Self::Order) -> Result<Self, secp256k1::Error> {
        order.try_into()
    }

    fn to_signed(self) -> Self::Order {
        self.signed_order
    }
}

impl OrderConversion for EcRecoveredComposableLimitOrder {
    type Order = SignedComposableLimitOrder;

    fn try_from_order(order: Self::Order) -> Result<Self, secp256k1::Error> {
        order.try_into()
    }

    fn to_signed(self) -> Self::Order {
        self.signed_order
    }
}

pub trait PooledLimitOrder: PoolOrder {
    /// The liquidity pool this order trades in
    fn pool_and_direction(&self) -> (PoolId, bool);
    fn gas(&self) -> u128;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrderPriorityData {
    pub price:  u128,
    pub volume: u128,
    pub gas:    u128
}

impl PartialOrd for OrderPriorityData {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.price.cmp(&other.price).then_with(|| {
            self.volume
                .cmp(&other.volume)
                .then_with(|| self.gas.cmp(&other.gas))
        }))
    }
}

impl Ord for OrderPriorityData {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.partial_cmp(other).unwrap()
    }
}

impl<O> ValidatedOrder<O, OrderPriorityData>
where
    O: PoolOrder
{
    pub fn pool_id(&self) -> usize {
        self.pool_id
    }

    pub fn is_bid(&self) -> bool {
        self.is_bid
    }

    pub fn priority_data(&self) -> OrderPriorityData {
        self.data
    }
}

impl PoolOrder for EcRecoveredLimitOrder {
    type ValidationData = OrderPriorityData;

    fn is_valid(&self) -> bool {
        todo!()
    }

    fn is_bid(&self) -> bool {
        todo!()
    }

    fn token_in(&self) -> Address {
        todo!()
    }

    fn token_out(&self) -> Address {
        todo!()
    }

    fn hash(&self) -> TxHash {
        self.signed_order.hash
    }

    fn from(&self) -> Address {
        self.signer
    }

    fn nonce(&self) -> U256 {
        self.order.nonce
    }

    fn amount_in(&self) -> u128 {
        self.signed_order.order.amountIn
    }

    fn amount_out_min(&self) -> u128 {
        self.signed_order.order.amountOutMin
    }

    fn limit_price(&self) -> u128 {
        self.amount_out_min() / self.amount_in()
    }

    fn deadline(&self) -> U256 {
        self.signed_order.order.deadline
    }

    fn size(&self) -> usize {
        unreachable!()
    }

    fn encoded_length(&self) -> usize {
        unreachable!()
    }

    fn chain_id(&self) -> Option<u64> {
        unreachable!()
    }
}

impl PooledLimitOrder for EcRecoveredLimitOrder {
    fn gas(&self) -> u128 {
        todo!()
    }

    fn pool_and_direction(&self) -> (PoolId, bool) {
        //(self.signed_order.order.pool, self.signed_order.order.direction)
        todo!()
    }
}

impl PoolOrder for EcRecoveredComposableLimitOrder {
    type ValidationData = OrderPriorityData;

    fn is_valid(&self) -> bool {
        todo!()
    }

    fn token_out(&self) -> Address {
        todo!()
    }

    fn is_bid(&self) -> bool {
        todo!()
    }

    fn hash(&self) -> TxHash {
        self.signed_order.hash
    }

    fn from(&self) -> Address {
        self.signer
    }

    fn nonce(&self) -> U256 {
        self.order.nonce
    }

    fn amount_in(&self) -> u128 {
        self.signed_order.order.amountIn
    }

    fn amount_out_min(&self) -> u128 {
        self.signed_order.order.amountOutMin
    }

    fn limit_price(&self) -> u128 {
        self.amount_out_min() / self.amount_in()
    }

    fn deadline(&self) -> U256 {
        self.signed_order.order.deadline
    }

    fn size(&self) -> usize {
        unreachable!()
    }

    fn encoded_length(&self) -> usize {
        unreachable!()
    }

    fn chain_id(&self) -> Option<u64> {
        unreachable!()
    }

    fn token_in(&self) -> Address {
        todo!()
    }
}

impl PooledLimitOrder for EcRecoveredComposableLimitOrder {
    fn gas(&self) -> u128 {
        todo!()
    }

    fn pool_and_direction(&self) -> (usize, bool) {
        //(self.signed_order.order.pool, self.signed_order.order.direction)
        todo!()
    }
}

impl PooledComposableOrder for EcRecoveredComposableLimitOrder {
    fn pre_hook(&self) -> Option<Bytes> {
        todo!()
    }

    fn post_hook(&self) -> Option<Bytes> {
        todo!()
    }
}
