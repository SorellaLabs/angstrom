use std::{hash::Hash, ops::Deref};

use alloy::primitives::{Address, Bytes, FixedBytes, TxHash, U256};
use reth_primitives::B256;
use serde::{Deserialize, Serialize};

use super::{RawPoolOrder, RespendAvoidanceMethod};
use crate::{
    matching::Ray,
    orders::{OrderId, OrderLocation, OrderPriorityData},
    primitive::{PoolId, Signature, ANGSTROM_DOMAIN},
    sol_bindings::rpc_orders::{
        ExactFlashOrder, ExactStandingOrder, OmitOrderMeta, PartialFlashOrder,
        PartialStandingOrder, TopOfBlockOrder
    }
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum AllOrders {
    Standing(StandingVariants),
    Flash(FlashVariants),
    TOB(crate::sol_bindings::rpc_orders::TopOfBlockOrder)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum StandingVariants {
    Partial(PartialStandingOrder),
    Exact(ExactStandingOrder)
}

impl StandingVariants {
    pub fn signature(&self) -> &Bytes {
        match self {
            StandingVariants::Exact(o) => &o.meta.signature,
            StandingVariants::Partial(o) => &o.meta.signature
        }
    }

    pub fn hook_data(&self) -> &Bytes {
        match self {
            StandingVariants::Exact(o) => &o.hookPayload,
            StandingVariants::Partial(o) => &o.hookPayload
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum FlashVariants {
    Partial(PartialFlashOrder),
    Exact(ExactFlashOrder)
}

impl FlashVariants {
    pub fn signature(&self) -> &Bytes {
        match self {
            FlashVariants::Exact(o) => &o.meta.signature,
            FlashVariants::Partial(o) => &o.meta.signature
        }
    }

    pub fn hook_data(&self) -> &Bytes {
        match self {
            FlashVariants::Exact(o) => &o.hookPayload,
            FlashVariants::Partial(o) => &o.hookPayload
        }
    }
}

impl From<TopOfBlockOrder> for AllOrders {
    fn from(value: TopOfBlockOrder) -> Self {
        Self::TOB(value)
    }
}
impl From<GroupedComposableOrder> for AllOrders {
    fn from(value: GroupedComposableOrder) -> Self {
        match value {
            GroupedComposableOrder::Partial(p) => AllOrders::Standing(p),
            GroupedComposableOrder::KillOrFill(kof) => AllOrders::Flash(kof)
        }
    }
}

impl From<GroupedVanillaOrder> for AllOrders {
    fn from(value: GroupedVanillaOrder) -> Self {
        match value {
            GroupedVanillaOrder::Standing(p) => AllOrders::Standing(p),
            GroupedVanillaOrder::KillOrFill(kof) => AllOrders::Flash(kof)
        }
    }
}

impl From<GroupedUserOrder> for AllOrders {
    fn from(value: GroupedUserOrder) -> Self {
        match value {
            GroupedUserOrder::Vanilla(v) => match v {
                GroupedVanillaOrder::Standing(p) => AllOrders::Standing(p),
                GroupedVanillaOrder::KillOrFill(kof) => AllOrders::Flash(kof)
            },
            GroupedUserOrder::Composable(v) => match v {
                GroupedComposableOrder::Partial(p) => AllOrders::Standing(p),
                GroupedComposableOrder::KillOrFill(kof) => AllOrders::Flash(kof)
            }
        }
    }
}

impl AllOrders {
    pub fn order_hash(&self) -> FixedBytes<32> {
        match self {
            Self::Standing(p) => match p {
                StandingVariants::Exact(e) => e.eip712_hash_struct(),
                StandingVariants::Partial(e) => e.eip712_hash_struct()
            },
            Self::Flash(f) => match f {
                FlashVariants::Exact(e) => e.eip712_hash_struct(),
                FlashVariants::Partial(e) => e.eip712_hash_struct()
            },
            Self::TOB(t) => t.eip712_hash_struct()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderWithStorageData<Order> {
    /// raw order
    pub order:              Order,
    /// the raw data needed for indexing the data
    pub priority_data:      OrderPriorityData,
    /// orders that this order invalidates. this occurs due to live nonce
    /// ordering
    pub invalidates:        Vec<B256>,
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

impl<Order> Hash for OrderWithStorageData<Order> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.order_id.hash(state)
    }
}

impl OrderWithStorageData<AllOrders> {
    pub fn from(&self) -> Address {
        match &self.order {
            AllOrders::Flash(kof) => match kof {
                FlashVariants::Exact(e) => e.meta.from,
                FlashVariants::Partial(p) => p.meta.from
            },
            AllOrders::Standing(p) => match p {
                StandingVariants::Partial(p) => p.meta.from,
                StandingVariants::Exact(p) => p.meta.from
            },
            AllOrders::TOB(tob) => tob.meta.from
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
            invalidates:        self.invalidates,
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

    pub fn is_composable(&self) -> bool {
        matches!(self, Self::Composable(_))
    }

    pub fn order_hash(&self) -> B256 {
        match self {
            GroupedUserOrder::Vanilla(v) => v.hash(),
            GroupedUserOrder::Composable(c) => c.hash()
        }
    }
}

impl RawPoolOrder for StandingVariants {
    fn token_out(&self) -> Address {
        match self {
            StandingVariants::Exact(e) => e.token_out(),
            StandingVariants::Partial(p) => p.token_out()
        }
    }

    fn token_in(&self) -> Address {
        match self {
            StandingVariants::Exact(e) => e.token_in(),
            StandingVariants::Partial(p) => p.token_in()
        }
    }

    fn order_hash(&self) -> TxHash {
        match self {
            StandingVariants::Exact(e) => e.order_hash(),
            StandingVariants::Partial(p) => p.order_hash()
        }
    }

    fn from(&self) -> Address {
        match self {
            StandingVariants::Exact(e) => e.meta.from,
            StandingVariants::Partial(p) => p.meta.from
        }
    }

    fn respend_avoidance_strategy(&self) -> RespendAvoidanceMethod {
        match self {
            StandingVariants::Exact(e) => e.respend_avoidance_strategy(),
            StandingVariants::Partial(p) => p.respend_avoidance_strategy()
        }
    }

    fn deadline(&self) -> Option<U256> {
        match self {
            StandingVariants::Exact(e) => e.deadline(),
            StandingVariants::Partial(p) => p.deadline()
        }
    }

    fn amount_in(&self) -> u128 {
        match self {
            StandingVariants::Exact(e) => e.amount_in(),
            StandingVariants::Partial(p) => p.amount_in()
        }
    }

    fn limit_price(&self) -> U256 {
        match self {
            StandingVariants::Exact(e) => e.limit_price(),
            StandingVariants::Partial(p) => p.limit_price()
        }
    }

    fn amount_out_min(&self) -> u128 {
        match self {
            StandingVariants::Exact(e) => e.amount_out_min(),
            StandingVariants::Partial(p) => p.amount_out_min()
        }
    }

    fn flash_block(&self) -> Option<u64> {
        None
    }

    fn is_valid_signature(&self) -> bool {
        match self {
            StandingVariants::Exact(e) => e.is_valid_signature(),
            StandingVariants::Partial(p) => p.is_valid_signature()
        }
    }

    fn order_location(&self) -> OrderLocation {
        OrderLocation::Limit
    }
}

impl RawPoolOrder for FlashVariants {
    fn is_valid_signature(&self) -> bool {
        match self {
            FlashVariants::Exact(e) => e.is_valid_signature(),
            FlashVariants::Partial(p) => p.is_valid_signature()
        }
    }

    fn order_hash(&self) -> TxHash {
        match self {
            FlashVariants::Exact(e) => e.order_hash(),
            FlashVariants::Partial(p) => p.order_hash()
        }
    }

    fn from(&self) -> Address {
        match self {
            FlashVariants::Exact(e) => e.meta.from,
            FlashVariants::Partial(p) => p.meta.from
        }
    }

    fn respend_avoidance_strategy(&self) -> RespendAvoidanceMethod {
        match self {
            FlashVariants::Exact(e) => e.respend_avoidance_strategy(),
            FlashVariants::Partial(p) => p.respend_avoidance_strategy()
        }
    }

    fn deadline(&self) -> Option<U256> {
        match self {
            FlashVariants::Exact(e) => e.deadline(),
            FlashVariants::Partial(p) => p.deadline()
        }
    }

    fn amount_in(&self) -> u128 {
        match self {
            FlashVariants::Exact(e) => e.amount_in(),
            FlashVariants::Partial(p) => p.amount_in()
        }
    }

    fn limit_price(&self) -> U256 {
        match self {
            FlashVariants::Exact(e) => e.limit_price(),
            FlashVariants::Partial(p) => p.limit_price()
        }
    }

    fn amount_out_min(&self) -> u128 {
        match self {
            FlashVariants::Exact(e) => e.amount_out_min(),
            FlashVariants::Partial(p) => p.amount_out_min()
        }
    }

    fn token_out(&self) -> Address {
        match self {
            FlashVariants::Exact(e) => e.token_out(),
            FlashVariants::Partial(p) => p.token_out()
        }
    }

    fn token_in(&self) -> Address {
        match self {
            FlashVariants::Exact(e) => e.token_in(),
            FlashVariants::Partial(p) => p.token_in()
        }
    }

    fn flash_block(&self) -> Option<u64> {
        match self {
            FlashVariants::Exact(e) => e.flash_block(),
            FlashVariants::Partial(p) => p.flash_block()
        }
    }

    fn order_location(&self) -> OrderLocation {
        OrderLocation::Limit
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GroupedVanillaOrder {
    Standing(StandingVariants),
    KillOrFill(FlashVariants)
}

impl GroupedVanillaOrder {
    pub fn hash(&self) -> FixedBytes<32> {
        match self {
            GroupedVanillaOrder::Standing(p) => p.order_hash(),
            GroupedVanillaOrder::KillOrFill(p) => p.order_hash()
        }
    }

    /// Primarily used for debugging to work with price as an f64
    pub fn float_price(&self) -> f64 {
        match self {
            Self::Standing(o) => Ray::from(o.limit_price()).as_f64(),
            Self::KillOrFill(o) => Ray::from(o.limit_price()).as_f64()
        }
    }

    pub fn price(&self) -> Ray {
        match self {
            Self::Standing(o) => o.limit_price().into(),
            Self::KillOrFill(o) => o.limit_price().into()
        }
    }

    pub fn quantity(&self) -> U256 {
        match self {
            Self::Standing(o) => U256::from(o.amount_in()),
            Self::KillOrFill(o) => U256::from(o.amount_in())
        }
    }

    pub fn fill(&self, filled_quantity: U256) -> Self {
        match self {
            Self::Standing(p) => match p {
                StandingVariants::Partial(part) => {
                    Self::Standing(StandingVariants::Partial(PartialStandingOrder {
                        amountFilled: filled_quantity.to(),
                        ..part.clone()
                    }))
                }
                v => Self::Standing(v.clone())
            },
            Self::KillOrFill(kof) => match kof {
                FlashVariants::Partial(part) => {
                    Self::KillOrFill(FlashVariants::Partial(PartialFlashOrder {
                        amountFilled: filled_quantity.to(),
                        ..part.clone()
                    }))
                }
                e => Self::KillOrFill(e.clone())
            }
        }
    }

    pub fn signature(&self) -> &Bytes {
        match self {
            Self::Standing(o) => o.signature(),
            Self::KillOrFill(o) => o.signature()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GroupedComposableOrder {
    Partial(StandingVariants),
    KillOrFill(FlashVariants)
}

impl GroupedComposableOrder {
    pub fn hash(&self) -> B256 {
        match self {
            Self::Partial(p) => match p {
                StandingVariants::Partial(p) => p.eip712_hash_struct(),
                StandingVariants::Exact(e) => e.eip712_hash_struct()
            },
            Self::KillOrFill(k) => match k {
                FlashVariants::Partial(p) => p.eip712_hash_struct(),
                FlashVariants::Exact(e) => e.eip712_hash_struct()
            }
        }
    }
}

impl RawPoolOrder for TopOfBlockOrder {
    fn flash_block(&self) -> Option<u64> {
        Some(self.validForBlock)
    }

    fn from(&self) -> Address {
        self.meta.from
    }

    fn order_hash(&self) -> TxHash {
        self.eip712_hash_struct()
    }

    fn respend_avoidance_strategy(&self) -> RespendAvoidanceMethod {
        RespendAvoidanceMethod::Block(self.validForBlock)
    }

    fn deadline(&self) -> Option<U256> {
        None
    }

    fn amount_in(&self) -> u128 {
        self.quantityIn
    }

    fn limit_price(&self) -> U256 {
        U256::from(self.amount_in() / self.amount_out_min())
    }

    fn amount_out_min(&self) -> u128 {
        self.quantityOut
    }

    fn token_in(&self) -> Address {
        self.assetIn
    }

    fn token_out(&self) -> Address {
        self.assetOut
    }

    fn is_valid_signature(&self) -> bool {
        let Ok(sig) = Signature::new_from_bytes(&self.meta.signature) else { return false };
        let hash = self.no_meta_eip712_signing_hash(&ANGSTROM_DOMAIN);
        sig.recover_signer(hash)
            .filter(|recovered_addr| recovered_addr == &self.meta.from)
            .is_some()
    }

    fn order_location(&self) -> OrderLocation {
        OrderLocation::Searcher
    }
}
impl RawPoolOrder for PartialStandingOrder {
    fn is_valid_signature(&self) -> bool {
        let Ok(sig) = Signature::new_from_bytes(&self.meta.signature) else { return false };
        let hash = self.no_meta_eip712_signing_hash(&ANGSTROM_DOMAIN);
        sig.recover_signer(hash)
            .filter(|recovered_addr| recovered_addr == &self.meta.from)
            .is_some()
    }

    fn flash_block(&self) -> Option<u64> {
        None
    }

    fn respend_avoidance_strategy(&self) -> RespendAvoidanceMethod {
        RespendAvoidanceMethod::Nonce(self.nonce)
    }

    fn amount_out_min(&self) -> u128 {
        self.amountFilled
    }

    fn limit_price(&self) -> U256 {
        self.minPrice
    }

    fn amount_in(&self) -> u128 {
        self.maxAmountIn
    }

    fn deadline(&self) -> Option<U256> {
        Some(U256::from(self.deadline))
    }

    fn from(&self) -> Address {
        self.meta.from
    }

    fn order_hash(&self) -> TxHash {
        self.eip712_hash_struct()
    }

    fn token_in(&self) -> Address {
        self.assetIn
    }

    fn token_out(&self) -> Address {
        self.assetOut
    }

    fn order_location(&self) -> OrderLocation {
        OrderLocation::Limit
    }
}

impl RawPoolOrder for ExactStandingOrder {
    fn is_valid_signature(&self) -> bool {
        let Ok(sig) = Signature::new_from_bytes(&self.meta.signature) else { return false };
        let hash = self.no_meta_eip712_signing_hash(&ANGSTROM_DOMAIN);
        sig.recover_signer(hash)
            .filter(|recovered_addr| recovered_addr == &self.meta.from)
            .is_some()
    }

    fn flash_block(&self) -> Option<u64> {
        None
    }

    fn respend_avoidance_strategy(&self) -> RespendAvoidanceMethod {
        RespendAvoidanceMethod::Nonce(self.nonce)
    }

    fn amount_out_min(&self) -> u128 {
        todo!();
        // self.amount * self.minPrice.to::<u128>()
    }

    fn limit_price(&self) -> U256 {
        self.minPrice
    }

    fn amount_in(&self) -> u128 {
        todo!();
        // self.amount
    }

    fn deadline(&self) -> Option<U256> {
        Some(U256::from(self.deadline))
    }

    fn from(&self) -> Address {
        self.meta.from
    }

    fn order_hash(&self) -> TxHash {
        self.eip712_hash_struct()
    }

    fn token_in(&self) -> Address {
        self.assetIn
    }

    fn token_out(&self) -> Address {
        self.assetOut
    }

    fn order_location(&self) -> OrderLocation {
        OrderLocation::Limit
    }
}

impl RawPoolOrder for PartialFlashOrder {
    fn is_valid_signature(&self) -> bool {
        let Ok(sig) = Signature::new_from_bytes(&self.meta.signature) else { return false };
        let hash = self.no_meta_eip712_signing_hash(&ANGSTROM_DOMAIN);
        sig.recover_signer(hash)
            .filter(|recovered_addr| recovered_addr == &self.meta.from)
            .is_some()
    }

    fn flash_block(&self) -> Option<u64> {
        Some(self.validForBlock)
    }

    fn order_hash(&self) -> TxHash {
        self.eip712_hash_struct()
    }

    fn from(&self) -> Address {
        self.meta.from
    }

    fn deadline(&self) -> Option<U256> {
        None
    }

    fn amount_in(&self) -> u128 {
        self.maxAmountIn
    }

    fn limit_price(&self) -> U256 {
        self.minPrice
    }

    fn amount_out_min(&self) -> u128 {
        self.minPrice.to::<u128>() * self.minAmountIn
    }

    fn respend_avoidance_strategy(&self) -> RespendAvoidanceMethod {
        RespendAvoidanceMethod::Block(self.validForBlock)
    }

    fn token_in(&self) -> Address {
        self.assetIn
    }

    fn token_out(&self) -> Address {
        self.assetOut
    }

    fn order_location(&self) -> OrderLocation {
        OrderLocation::Limit
    }
}

impl RawPoolOrder for ExactFlashOrder {
    fn is_valid_signature(&self) -> bool {
        let Ok(sig) = Signature::new_from_bytes(&self.meta.signature) else { return false };
        let hash = self.no_meta_eip712_signing_hash(&ANGSTROM_DOMAIN);
        sig.recover_signer(hash)
            .filter(|recovered_addr| recovered_addr == &self.meta.from)
            .is_some()
    }

    fn flash_block(&self) -> Option<u64> {
        Some(self.validForBlock)
    }

    fn token_in(&self) -> Address {
        self.assetIn
    }

    fn token_out(&self) -> Address {
        self.assetOut
    }

    fn order_hash(&self) -> TxHash {
        self.eip712_hash_struct()
    }

    fn from(&self) -> Address {
        self.meta.from
    }

    fn deadline(&self) -> Option<U256> {
        None
    }

    fn amount_in(&self) -> u128 {
        self.amount
    }

    fn limit_price(&self) -> U256 {
        self.minPrice
    }

    fn amount_out_min(&self) -> u128 {
        self.minPrice.to::<u128>() * self.amount
    }

    fn respend_avoidance_strategy(&self) -> RespendAvoidanceMethod {
        RespendAvoidanceMethod::Block(self.validForBlock)
    }

    fn order_location(&self) -> OrderLocation {
        OrderLocation::Limit
    }
}

impl RawPoolOrder for AllOrders {
    fn is_valid_signature(&self) -> bool {
        match self {
            AllOrders::Standing(p) => p.is_valid_signature(),
            AllOrders::Flash(kof) => kof.is_valid_signature(),
            AllOrders::TOB(tob) => tob.is_valid_signature()
        }
    }

    fn from(&self) -> Address {
        match self {
            AllOrders::Standing(p) => p.from(),
            AllOrders::Flash(kof) => kof.from(),
            AllOrders::TOB(tob) => tob.from()
        }
    }

    fn order_hash(&self) -> TxHash {
        match self {
            AllOrders::Standing(p) => p.order_hash(),
            AllOrders::Flash(kof) => kof.order_hash(),
            AllOrders::TOB(tob) => tob.order_hash()
        }
    }

    fn respend_avoidance_strategy(&self) -> RespendAvoidanceMethod {
        match self {
            AllOrders::Standing(p) => p.respend_avoidance_strategy(),
            AllOrders::Flash(kof) => kof.respend_avoidance_strategy(),
            AllOrders::TOB(tob) => tob.respend_avoidance_strategy()
        }
    }

    fn deadline(&self) -> Option<U256> {
        match self {
            AllOrders::Standing(p) => p.deadline(),
            AllOrders::Flash(k) => k.deadline(),
            AllOrders::TOB(t) => t.deadline()
        }
    }

    fn amount_in(&self) -> u128 {
        match self {
            AllOrders::Standing(p) => p.amount_in(),
            AllOrders::Flash(kof) => kof.amount_in(),
            AllOrders::TOB(tob) => tob.amount_in()
        }
    }

    fn limit_price(&self) -> U256 {
        match self {
            AllOrders::Standing(p) => p.limit_price(),
            AllOrders::Flash(kof) => kof.limit_price(),
            AllOrders::TOB(t) => t.limit_price()
        }
    }

    fn amount_out_min(&self) -> u128 {
        match self {
            AllOrders::Standing(p) => p.amount_out_min(),
            AllOrders::Flash(kof) => kof.amount_out_min(),
            AllOrders::TOB(tob) => tob.amount_out_min()
        }
    }

    fn token_out(&self) -> Address {
        match self {
            AllOrders::Standing(p) => p.token_out(),
            AllOrders::Flash(kof) => kof.token_out(),
            AllOrders::TOB(tob) => tob.token_out()
        }
    }

    fn token_in(&self) -> Address {
        match self {
            AllOrders::Standing(p) => p.token_in(),
            AllOrders::Flash(kof) => kof.token_in(),
            AllOrders::TOB(tob) => tob.token_in()
        }
    }

    fn flash_block(&self) -> Option<u64> {
        match self {
            AllOrders::Standing(_) => None,
            AllOrders::Flash(kof) => kof.flash_block(),
            AllOrders::TOB(tob) => tob.flash_block()
        }
    }

    fn order_location(&self) -> OrderLocation {
        match &self {
            AllOrders::Standing(_) => OrderLocation::Limit,
            AllOrders::Flash(_) => OrderLocation::Limit,
            AllOrders::TOB(_) => OrderLocation::Searcher
        }
    }
}

impl RawPoolOrder for GroupedVanillaOrder {
    fn is_valid_signature(&self) -> bool {
        match self {
            GroupedVanillaOrder::Standing(p) => p.is_valid_signature(),
            GroupedVanillaOrder::KillOrFill(kof) => kof.is_valid_signature()
        }
    }

    fn respend_avoidance_strategy(&self) -> RespendAvoidanceMethod {
        match self {
            GroupedVanillaOrder::Standing(p) => p.respend_avoidance_strategy(),
            GroupedVanillaOrder::KillOrFill(kof) => kof.respend_avoidance_strategy()
        }
    }

    fn flash_block(&self) -> Option<u64> {
        match self {
            GroupedVanillaOrder::Standing(_) => None,
            GroupedVanillaOrder::KillOrFill(kof) => kof.flash_block()
        }
    }

    fn token_in(&self) -> Address {
        match self {
            GroupedVanillaOrder::Standing(p) => p.token_in(),
            GroupedVanillaOrder::KillOrFill(kof) => kof.token_in()
        }
    }

    fn token_out(&self) -> Address {
        match self {
            GroupedVanillaOrder::Standing(p) => p.token_out(),
            GroupedVanillaOrder::KillOrFill(kof) => kof.token_out()
        }
    }

    fn from(&self) -> Address {
        match self {
            GroupedVanillaOrder::Standing(p) => p.from(),
            GroupedVanillaOrder::KillOrFill(kof) => kof.from()
        }
    }

    fn order_hash(&self) -> TxHash {
        match self {
            GroupedVanillaOrder::Standing(p) => p.order_hash(),
            GroupedVanillaOrder::KillOrFill(kof) => kof.order_hash()
        }
    }

    fn deadline(&self) -> Option<U256> {
        match self {
            GroupedVanillaOrder::Standing(p) => p.deadline(),
            GroupedVanillaOrder::KillOrFill(kof) => kof.deadline()
        }
    }

    fn amount_in(&self) -> u128 {
        match self {
            GroupedVanillaOrder::Standing(p) => p.amount_in(),
            GroupedVanillaOrder::KillOrFill(kof) => kof.amount_in()
        }
    }

    fn limit_price(&self) -> U256 {
        match self {
            GroupedVanillaOrder::Standing(p) => p.limit_price(),
            GroupedVanillaOrder::KillOrFill(p) => p.limit_price()
        }
    }

    fn amount_out_min(&self) -> u128 {
        match self {
            GroupedVanillaOrder::Standing(p) => p.amount_out_min(),
            GroupedVanillaOrder::KillOrFill(kof) => kof.amount_out_min()
        }
    }

    fn order_location(&self) -> OrderLocation {
        match &self {
            GroupedVanillaOrder::Standing(_) => OrderLocation::Limit,
            GroupedVanillaOrder::KillOrFill(_) => OrderLocation::Limit
        }
    }
}

impl RawPoolOrder for GroupedComposableOrder {
    fn flash_block(&self) -> Option<u64> {
        match self {
            GroupedComposableOrder::Partial(_) => None,
            GroupedComposableOrder::KillOrFill(kof) => kof.flash_block()
        }
    }

    fn respend_avoidance_strategy(&self) -> RespendAvoidanceMethod {
        match self {
            GroupedComposableOrder::Partial(p) => p.respend_avoidance_strategy(),
            GroupedComposableOrder::KillOrFill(kof) => kof.respend_avoidance_strategy()
        }
    }

    fn token_in(&self) -> Address {
        match self {
            GroupedComposableOrder::Partial(p) => p.token_in(),
            GroupedComposableOrder::KillOrFill(kof) => kof.token_in()
        }
    }

    fn token_out(&self) -> Address {
        match self {
            GroupedComposableOrder::Partial(p) => p.token_out(),
            GroupedComposableOrder::KillOrFill(kof) => kof.token_out()
        }
    }

    fn from(&self) -> Address {
        match self {
            GroupedComposableOrder::Partial(p) => p.from(),
            GroupedComposableOrder::KillOrFill(kof) => kof.from()
        }
    }

    fn order_hash(&self) -> TxHash {
        match self {
            GroupedComposableOrder::Partial(p) => p.order_hash(),
            GroupedComposableOrder::KillOrFill(kof) => kof.order_hash()
        }
    }

    fn deadline(&self) -> Option<U256> {
        match self {
            GroupedComposableOrder::Partial(p) => p.deadline(),
            GroupedComposableOrder::KillOrFill(kof) => kof.deadline()
        }
    }

    fn amount_in(&self) -> u128 {
        match self {
            GroupedComposableOrder::Partial(p) => p.amount_in(),
            GroupedComposableOrder::KillOrFill(kof) => kof.amount_in()
        }
    }

    fn limit_price(&self) -> U256 {
        match self {
            GroupedComposableOrder::Partial(p) => p.limit_price(),
            GroupedComposableOrder::KillOrFill(p) => p.limit_price()
        }
    }

    fn amount_out_min(&self) -> u128 {
        match self {
            GroupedComposableOrder::Partial(p) => p.amount_out_min(),
            GroupedComposableOrder::KillOrFill(kof) => kof.amount_out_min()
        }
    }

    fn is_valid_signature(&self) -> bool {
        match self {
            GroupedComposableOrder::Partial(p) => p.is_valid_signature(),
            GroupedComposableOrder::KillOrFill(kof) => kof.is_valid_signature()
        }
    }

    fn order_location(&self) -> OrderLocation {
        match &self {
            GroupedComposableOrder::Partial(_) => OrderLocation::Limit,
            GroupedComposableOrder::KillOrFill(_) => OrderLocation::Limit
        }
    }
}
