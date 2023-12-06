use alloy_primitives::{Address, Bytes, TxHash, U256};

use super::{
    super::{PoolOrder, PooledComposableOrder},
    ValidatedOrder
};
use crate::rpc::{
    EcRecoveredComposableSearcherOrder, EcRecoveredSearcherOrder, SignedComposableSearcherOrder,
    SignedSearcherOrder
};

pub trait FromSignedSearcherOrder {
    fn from_signed_searcher_order(tx: SignedSearcherOrder) -> Self;
}

impl FromSignedSearcherOrder for EcRecoveredSearcherOrder {
    fn from_signed_searcher_order(tx: SignedSearcherOrder) -> Self {
        tx.try_into().unwrap()
    }
}

pub trait FromSignedComposableSearcherOrder {
    fn from_signed_composable_searcher_order(tx: SignedComposableSearcherOrder) -> Self;
}

impl FromSignedComposableSearcherOrder for EcRecoveredComposableSearcherOrder {
    fn from_signed_composable_searcher_order(tx: SignedComposableSearcherOrder) -> Self {
        tx.try_into().unwrap()
    }
}

pub trait FromSearcherOrder {
    fn from_searcher(&self) -> SignedSearcherOrder;
}

impl FromSearcherOrder for EcRecoveredSearcherOrder {
    fn from_searcher(&self) -> SignedSearcherOrder {
        self.signed_order.clone()
    }
}

pub trait FromComposableSearcherOrder {
    fn from_composable_searcher(&self) -> SignedComposableSearcherOrder;
}

impl FromComposableSearcherOrder for EcRecoveredComposableSearcherOrder {
    fn from_composable_searcher(&self) -> SignedComposableSearcherOrder {
        self.signed_order.clone()
    }
}

pub trait PooledSearcherOrder: PoolOrder {
    /// The liquidity pool this order trades in
    fn pool(&self) -> u8;
    /// donate value
    fn donate(&self) -> (u128, u128);

    fn volume(&self) -> u128;

    fn gas(&self) -> u128;

    fn donated(&self) -> u128;
}

impl<O> ValidatedOrder<O, SearcherPriorityData>
where
    O: PooledSearcherOrder
{
    pub fn pool_id(&self) -> usize {
        self.pool_id
    }

    pub fn is_bid(&self) -> bool {
        self.is_bid
    }

    pub fn priority_data(&self) -> SearcherPriorityData {
        self.data.clone()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearcherPriorityData {
    pub donated: u128,
    pub volume:  u128,
    pub gas:     u128
}

/// Reverse ordering for arb priority data to sort donated value in descending
/// order
impl PartialOrd for SearcherPriorityData {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(
            other
                .donated
                .cmp(&self.donated)
                .then_with(|| other.volume.cmp(&self.volume))
        )
    }
}

impl Ord for SearcherPriorityData {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.partial_cmp(other).unwrap()
    }
}

impl PoolOrder for EcRecoveredSearcherOrder {
    type ValidationData = SearcherPriorityData;

    fn token_out(&self) -> Address {
        todo!()
    }

    fn is_valid(&self) -> bool {
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

impl PooledSearcherOrder for EcRecoveredSearcherOrder {
    fn gas(&self) -> u128 {
        todo!()
    }

    fn pool(&self) -> u8 {
        todo!()
    }

    fn donate(&self) -> (u128, u128) {
        todo!()
    }

    fn volume(&self) -> u128 {
        todo!()
    }

    fn donated(&self) -> u128 {
        todo!()
    }
}

impl PoolOrder for EcRecoveredComposableSearcherOrder {
    type ValidationData = ();

    fn token_out(&self) -> Address {
        todo!()
    }

    fn is_valid(&self) -> bool {
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

    fn token_in(&self) -> Address {
        todo!()
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

impl PooledSearcherOrder for EcRecoveredComposableSearcherOrder {
    fn gas(&self) -> u128 {
        todo!()
    }

    fn pool(&self) -> u8 {
        todo!()
    }

    fn donate(&self) -> (u128, u128) {
        todo!()
    }

    fn volume(&self) -> u128 {
        todo!()
    }

    fn donated(&self) -> u128 {
        todo!()
    }
}

impl PooledComposableOrder for EcRecoveredComposableSearcherOrder {
    fn pre_hook(&self) -> Option<Bytes> {
        todo!()
    }

    fn post_hook(&self) -> Option<Bytes> {
        todo!()
    }
}
