use std::{hash::Hash, ops::Deref};

use alloy::{
    primitives::{Address, Bytes, FixedBytes, TxHash, U256},
    signers::Signature
};
use alloy_primitives::{PrimitiveSignature, B256};
use pade::PadeDecode;
use serde::{Deserialize, Serialize};

use super::{GenerateFlippedOrder, RawPoolOrder, RespendAvoidanceMethod};
use crate::{
    matching::{Debt, Ray},
    orders::{OrderId, OrderLocation, OrderPriorityData},
    primitive::{PoolId, ANGSTROM_DOMAIN},
    sol_bindings::rpc_orders::{
        ExactFlashOrder, ExactStandingOrder, OmitOrderMeta, PartialFlashOrder,
        PartialStandingOrder, TopOfBlockOrder
    }
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum AllOrders {
    Standing(StandingVariants),
    Flash(FlashVariants),
    TOB(TopOfBlockOrder)
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
            StandingVariants::Exact(o) => &o.hook_data,
            StandingVariants::Partial(o) => &o.hook_data
        }
    }

    /// Maximum quantity accepted by this order
    pub fn max_q(&self) -> u128 {
        match self {
            Self::Exact(o) => o.amount,
            Self::Partial(o) => o.max_amount_in
        }
    }

    /// Minimum quantity accepted by this order
    pub fn min_q(&self) -> u128 {
        match self {
            Self::Exact(o) => o.amount,
            Self::Partial(o) => o.min_amount_in
        }
    }

    /// The quantity available for this order to match in terms of T0
    pub fn quantity(&self, _debt: Option<&Debt>) -> u128 {
        let is_bid = self.is_bid();
        let exact_in = self.exact_in();
        let raw_q = match self {
            Self::Exact(o) => o.amount,
            Self::Partial(o) => o.max_amount_in
        };
        match (is_bid, exact_in) {
            // Exact In bid
            (true, true) => {
                // In this case the price in this order is stored as T0/T1 so we use
                // mul_quantity to get the number of T0 for this amount of T1
                let order_price = Ray::from(self.limit_price());
                order_price.mul_quantity(U256::from(raw_q)).saturating_to()
            }
            // Exact Out ask
            (false, false) => {
                let order_price = Ray::from(self.limit_price());
                order_price.inverse_quantity(raw_q, true)
            }
            // Exact Out bid (normal bid) and Exact In ask (normal ask)
            (true, false) | (false, true) => raw_q
        }
    }

    pub fn exact_in(&self) -> bool {
        match self {
            StandingVariants::Exact(o) => o.exact_in,
            StandingVariants::Partial(_) => true
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
            FlashVariants::Exact(o) => &o.hook_data,
            FlashVariants::Partial(o) => &o.hook_data
        }
    }

    pub fn max_q(&self) -> u128 {
        match self {
            Self::Exact(o) => o.amount,
            Self::Partial(o) => o.max_amount_in
        }
    }

    pub fn min_q(&self) -> u128 {
        match self {
            Self::Exact(o) => o.amount,
            Self::Partial(o) => o.max_amount_in
        }
    }

    /// The quantity available for this order to match in terms of T0
    pub fn quantity(&self) -> u128 {
        let is_bid = self.is_bid();
        let exact_in = self.exact_in();
        let raw_q = match self {
            Self::Exact(o) => o.amount,
            Self::Partial(o) => o.max_amount_in
        };
        match (is_bid, exact_in) {
            // Exact In bid
            (true, true) => {
                // In this case the price is stored as T0/T1 so we use mul_quantity
                // to get the number of T0 for this amount of T1
                let order_price = Ray::from(self.limit_price());
                order_price.mul_quantity(U256::from(raw_q)).saturating_to()
            }
            // Exact Out ask
            (false, false) => {
                let order_price = Ray::from(self.limit_price());
                order_price.inverse_quantity(raw_q, true)
            }
            // Exact Out bid (normal bid) and Exact In ask (normal ask)
            (true, false) | (false, true) => raw_q
        }
    }

    pub fn exact_in(&self) -> bool {
        match self {
            FlashVariants::Exact(o) => o.exact_in,
            FlashVariants::Partial(_) => true
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
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
    pub order_id:           OrderId,
    pub tob_reward:         U256
}

impl<O: RawPoolOrder> OrderWithStorageData<O> {
    pub fn fetch_supply_or_demand_contribution_with_fee(&self, price: Ray, pool_fee: u32) -> Ray {
        // priceOutVsIn * oneMinusFee / ONE_E6;

        match (self.is_bid, self.exact_in()) {
            (true, true) => {
                // quantityIn = AmountIn.wrap(quantity);
                // quantityOut = price.convertDown(quantityIn) - fee;
                // .map(|bid|
                // Ray::from(U256::from(bid.amount())).div_ray(price))

                Ray::from(U256::from(price.inverse_quantity(self.amount(), false)))
                    - Ray::from(self.priority_data.gas)
            }
            (true, false) => {
                // quantityOut = AmountOut.wrap(quantity);
                // quantityIn = price.convertUp(quantityOut + fee);
                Ray::from(U256::from(self.amount()))
            }
            (false, true) => {
                // quantityIn = AmountIn.wrap(quantity);
                // quantityOut = price.convertDown(quantityIn - fee);

                // .map(|ask| Ray::from(U256::from(ask.amount())))
                Ray::from(U256::from(self.amount()))
            }

            (false, false) => {
                // quantityOut = AmountOut.wrap(quantity);
                // quantityIn = price.convertUp(quantityOut) + fee;
                //
                // .map(|ask|
                Ray::from(U256::from(price.inverse_quantity(self.amount(), true)))
                    + Ray::from(self.priority_data.gas)
            }
        }
    }

    pub fn fetch_supply_or_demand_contribution_with_fee_partial(
        &self,
        price: Ray,
        pool_fee: u32
    ) -> (Ray, Option<Ray>) {
        let limit_price = if self.is_bid {
            Ray::from(self.limit_price()).inv_ray_round(true)
        } else {
            Ray::from(self.limit_price())
        };

        let (max, min) = (self.amount(), self.min_amount());

        if limit_price == price {
            if self.is_bid {
                let high = Ray::from(U256::from(price.inverse_quantity(max, false)))
                    - Ray::from(self.priority_data.gas);

                let low = Ray::from(U256::from(price.inverse_quantity(min, false)))
                    - Ray::from(self.priority_data.gas);

                (high, Some(high - low))
            } else {
                let (max, min) = (
                    Ray::from(U256::from(self.amount())),
                    Ray::from(U256::from(self.min_amount()))
                );
                (max, Some(max - min))
            }
        } else {
            if self.is_bid {
                let high = Ray::from(U256::from(price.inverse_quantity(max, false)))
                    - Ray::from(self.priority_data.gas);

                (high, None)
            } else {
                (Ray::from(U256::from(self.amount())), None)
            }
        }
    }
}

impl<O: GenerateFlippedOrder> GenerateFlippedOrder for OrderWithStorageData<O> {
    fn flip(&self) -> Self
    where
        Self: Sized
    {
        Self { order: self.order.flip(), is_bid: !self.is_bid, ..self.clone() }
    }
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
            order_id:           self.order_id,
            tob_reward:         U256::ZERO
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
    fn min_amount(&self) -> u128 {
        match self {
            StandingVariants::Exact(e) => e.min_amount(),
            StandingVariants::Partial(p) => p.min_amount()
        }
    }

    fn exact_in(&self) -> bool {
        match self {
            StandingVariants::Exact(e) => e.exact_in(),
            StandingVariants::Partial(p) => p.exact_in()
        }
    }

    fn max_gas_token_0(&self) -> u128 {
        match self {
            StandingVariants::Exact(e) => e.max_gas_token_0(),
            StandingVariants::Partial(p) => p.max_gas_token_0()
        }
    }

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

    fn amount(&self) -> u128 {
        match self {
            StandingVariants::Exact(e) => e.amount(),
            StandingVariants::Partial(p) => p.amount()
        }
    }

    fn limit_price(&self) -> U256 {
        match self {
            StandingVariants::Exact(e) => e.limit_price(),
            StandingVariants::Partial(p) => p.limit_price()
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

    fn use_internal(&self) -> bool {
        match self {
            StandingVariants::Exact(e) => e.use_internal(),
            StandingVariants::Partial(p) => p.use_internal()
        }
    }

    fn order_signature(&self) -> eyre::Result<PrimitiveSignature> {
        match self {
            StandingVariants::Exact(e) => e.order_signature(),
            StandingVariants::Partial(p) => p.order_signature()
        }
    }
}

impl RawPoolOrder for FlashVariants {
    fn min_amount(&self) -> u128 {
        match self {
            FlashVariants::Exact(e) => e.min_amount(),
            FlashVariants::Partial(p) => p.min_amount()
        }
    }

    fn exact_in(&self) -> bool {
        match self {
            FlashVariants::Exact(e) => e.exact_in(),
            FlashVariants::Partial(p) => p.exact_in()
        }
    }

    fn max_gas_token_0(&self) -> u128 {
        match self {
            FlashVariants::Exact(e) => e.max_extra_fee_asset0,
            FlashVariants::Partial(p) => p.max_extra_fee_asset0
        }
    }

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

    fn amount(&self) -> u128 {
        match self {
            FlashVariants::Exact(e) => e.amount(),
            FlashVariants::Partial(p) => p.amount()
        }
    }

    fn limit_price(&self) -> U256 {
        match self {
            FlashVariants::Exact(e) => e.limit_price(),
            FlashVariants::Partial(p) => p.limit_price()
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

    fn use_internal(&self) -> bool {
        match self {
            FlashVariants::Exact(e) => e.use_internal(),
            FlashVariants::Partial(p) => p.use_internal()
        }
    }

    fn order_signature(&self) -> eyre::Result<PrimitiveSignature> {
        match self {
            FlashVariants::Exact(e) => e.order_signature(),
            FlashVariants::Partial(p) => p.order_signature()
        }
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum GroupedVanillaOrder {
    Standing(StandingVariants),
    KillOrFill(FlashVariants)
}

impl Default for GroupedVanillaOrder {
    fn default() -> Self {
        GroupedVanillaOrder::Standing(StandingVariants::Exact(ExactStandingOrder::default()))
    }
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

    /// Bid orders need to invert their price
    pub fn bid_price(&self) -> Ray {
        self.price().inv_ray_round(true)
    }

    /// Get the appropriate price when passed a bool telling us if we're looking
    /// for a bid-side price or not
    pub fn price_for_book_side(&self, is_bid: bool) -> Ray {
        if is_bid {
            self.bid_price()
        } else {
            self.price()
        }
    }

    /// Provides the LITERAL price as specified in the order.  Note that for
    /// bids this can be inverse
    pub fn price(&self) -> Ray {
        match self {
            Self::Standing(o) => o.limit_price().into(),
            Self::KillOrFill(o) => o.limit_price().into()
        }
    }

    pub fn exact_in(&self) -> bool {
        match self {
            Self::Standing(o) => o.exact_in(),
            Self::KillOrFill(o) => o.exact_in()
        }
    }

    /// Maximum quantity fillable by this order
    pub fn max_q(&self) -> u128 {
        match self {
            Self::Standing(o) => o.max_q(),
            Self::KillOrFill(o) => o.max_q()
        }
    }

    /// Quantity filled by this order in terms of T0
    pub fn quantity_t0(&self) -> u128 {
        0
    }

    pub fn signature(&self) -> &Bytes {
        match self {
            Self::Standing(o) => o.signature(),
            Self::KillOrFill(o) => o.signature()
        }
    }

    pub fn is_partial(&self) -> bool {
        matches!(
            self,
            Self::Standing(StandingVariants::Partial(_))
                | Self::KillOrFill(FlashVariants::Partial(_))
        )
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
    fn exact_in(&self) -> bool {
        true
    }

    fn min_amount(&self) -> u128 {
        self.quantity_in
    }

    fn max_gas_token_0(&self) -> u128 {
        self.max_gas_asset0
    }

    fn flash_block(&self) -> Option<u64> {
        Some(self.valid_for_block)
    }

    fn from(&self) -> Address {
        self.meta.from
    }

    fn order_hash(&self) -> TxHash {
        self.eip712_hash_struct()
    }

    fn respend_avoidance_strategy(&self) -> RespendAvoidanceMethod {
        RespendAvoidanceMethod::Block(self.valid_for_block)
    }

    fn deadline(&self) -> Option<U256> {
        None
    }

    fn amount(&self) -> u128 {
        self.quantity_in
    }

    fn limit_price(&self) -> U256 {
        *Ray::scale_to_ray(U256::from(self.amount() / self.quantity_out))
    }

    fn token_in(&self) -> Address {
        self.asset_in
    }

    fn token_out(&self) -> Address {
        self.asset_out
    }

    fn is_valid_signature(&self) -> bool {
        let Ok(sig) = self.order_signature() else { return false };
        let hash = self.no_meta_eip712_signing_hash(&ANGSTROM_DOMAIN);

        sig.recover_address_from_prehash(&hash)
            .map(|addr| addr == self.meta.from)
            .unwrap_or_default()
    }

    fn order_location(&self) -> OrderLocation {
        OrderLocation::Searcher
    }

    fn use_internal(&self) -> bool {
        self.use_internal
    }

    fn order_signature(&self) -> eyre::Result<PrimitiveSignature> {
        let s = self.meta.signature.to_vec();
        let mut slice = s.as_slice();

        Ok(Signature::pade_decode(&mut slice, None)?)
    }
}

impl RawPoolOrder for PartialStandingOrder {
    fn min_amount(&self) -> u128 {
        self.min_amount_in
    }

    fn exact_in(&self) -> bool {
        true
    }

    fn max_gas_token_0(&self) -> u128 {
        self.max_extra_fee_asset0
    }

    fn is_valid_signature(&self) -> bool {
        let s = self.meta.signature.to_vec();
        let mut slice = s.as_slice();

        let Ok(sig) = Signature::pade_decode(&mut slice, None) else { return false };
        let hash = self.no_meta_eip712_signing_hash(&ANGSTROM_DOMAIN);

        sig.recover_address_from_prehash(&hash)
            .map(|addr| addr == self.meta.from)
            .unwrap_or_default()
    }

    fn flash_block(&self) -> Option<u64> {
        None
    }

    fn respend_avoidance_strategy(&self) -> RespendAvoidanceMethod {
        RespendAvoidanceMethod::Nonce(self.nonce)
    }

    fn limit_price(&self) -> U256 {
        self.min_price
    }

    fn amount(&self) -> u128 {
        self.max_amount_in
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
        self.asset_in
    }

    fn token_out(&self) -> Address {
        self.asset_out
    }

    fn order_location(&self) -> OrderLocation {
        OrderLocation::Limit
    }

    fn use_internal(&self) -> bool {
        self.use_internal
    }

    fn order_signature(&self) -> eyre::Result<PrimitiveSignature> {
        let s = self.meta.signature.to_vec();
        let mut slice = s.as_slice();

        Ok(Signature::pade_decode(&mut slice, None)?)
    }
}

impl RawPoolOrder for ExactStandingOrder {
    fn min_amount(&self) -> u128 {
        self.amount
    }

    fn exact_in(&self) -> bool {
        self.exact_in
    }

    fn max_gas_token_0(&self) -> u128 {
        self.max_extra_fee_asset0
    }

    fn is_valid_signature(&self) -> bool {
        let s = self.meta.signature.to_vec();
        let mut slice = s.as_slice();

        let Ok(sig) = Signature::pade_decode(&mut slice, None) else { return false };
        let hash = self.no_meta_eip712_signing_hash(&ANGSTROM_DOMAIN);

        sig.recover_address_from_prehash(&hash)
            .map(|addr| addr == self.meta.from)
            .unwrap_or_default()
    }

    fn flash_block(&self) -> Option<u64> {
        None
    }

    fn respend_avoidance_strategy(&self) -> RespendAvoidanceMethod {
        RespendAvoidanceMethod::Nonce(self.nonce)
    }

    fn limit_price(&self) -> U256 {
        self.min_price
    }

    fn amount(&self) -> u128 {
        self.amount
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
        self.asset_in
    }

    fn token_out(&self) -> Address {
        self.asset_out
    }

    fn order_location(&self) -> OrderLocation {
        OrderLocation::Limit
    }

    fn use_internal(&self) -> bool {
        self.use_internal
    }

    fn order_signature(&self) -> eyre::Result<PrimitiveSignature> {
        let s = self.meta.signature.to_vec();
        let mut slice = s.as_slice();

        Ok(Signature::pade_decode(&mut slice, None)?)
    }
}

impl RawPoolOrder for PartialFlashOrder {
    fn min_amount(&self) -> u128 {
        self.min_amount_in
    }

    fn exact_in(&self) -> bool {
        true
    }

    fn max_gas_token_0(&self) -> u128 {
        self.max_extra_fee_asset0
    }

    fn is_valid_signature(&self) -> bool {
        let s = self.meta.signature.to_vec();
        let mut slice = s.as_slice();

        let Ok(sig) = Signature::pade_decode(&mut slice, None) else { return false };
        let hash = self.no_meta_eip712_signing_hash(&ANGSTROM_DOMAIN);

        sig.recover_address_from_prehash(&hash)
            .map(|addr| addr == self.meta.from)
            .unwrap_or_default()
    }

    fn flash_block(&self) -> Option<u64> {
        Some(self.valid_for_block)
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

    fn amount(&self) -> u128 {
        self.max_amount_in
    }

    fn limit_price(&self) -> U256 {
        self.min_price
    }

    fn respend_avoidance_strategy(&self) -> RespendAvoidanceMethod {
        RespendAvoidanceMethod::Block(self.valid_for_block)
    }

    fn token_in(&self) -> Address {
        self.asset_in
    }

    fn token_out(&self) -> Address {
        self.asset_out
    }

    fn order_location(&self) -> OrderLocation {
        OrderLocation::Limit
    }

    fn use_internal(&self) -> bool {
        self.use_internal
    }

    fn order_signature(&self) -> eyre::Result<PrimitiveSignature> {
        let s = self.meta.signature.to_vec();
        let mut slice = s.as_slice();

        Ok(Signature::pade_decode(&mut slice, None)?)
    }
}

impl RawPoolOrder for ExactFlashOrder {
    fn min_amount(&self) -> u128 {
        self.amount
    }

    fn exact_in(&self) -> bool {
        self.exact_in
    }

    fn max_gas_token_0(&self) -> u128 {
        self.max_extra_fee_asset0
    }

    fn is_valid_signature(&self) -> bool {
        let s = self.meta.signature.to_vec();
        let mut slice = s.as_slice();

        let Ok(sig) = Signature::pade_decode(&mut slice, None) else { return false };
        let hash = self.no_meta_eip712_signing_hash(&ANGSTROM_DOMAIN);

        sig.recover_address_from_prehash(&hash)
            .map(|addr| addr == self.meta.from)
            .unwrap_or_default()
    }

    fn flash_block(&self) -> Option<u64> {
        Some(self.valid_for_block)
    }

    fn token_in(&self) -> Address {
        self.asset_in
    }

    fn token_out(&self) -> Address {
        self.asset_out
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

    fn amount(&self) -> u128 {
        self.amount
    }

    fn limit_price(&self) -> U256 {
        self.min_price
    }

    fn respend_avoidance_strategy(&self) -> RespendAvoidanceMethod {
        RespendAvoidanceMethod::Block(self.valid_for_block)
    }

    fn order_location(&self) -> OrderLocation {
        OrderLocation::Limit
    }

    fn use_internal(&self) -> bool {
        self.use_internal
    }

    fn order_signature(&self) -> eyre::Result<PrimitiveSignature> {
        let s = self.meta.signature.to_vec();
        let mut slice = s.as_slice();

        Ok(Signature::pade_decode(&mut slice, None)?)
    }
}

impl RawPoolOrder for AllOrders {
    fn min_amount(&self) -> u128 {
        match self {
            AllOrders::Standing(p) => p.min_amount(),
            AllOrders::Flash(kof) => kof.min_amount(),
            AllOrders::TOB(tob) => tob.min_amount()
        }
    }

    fn exact_in(&self) -> bool {
        match self {
            AllOrders::Standing(p) => p.exact_in(),
            AllOrders::Flash(kof) => kof.exact_in(),
            AllOrders::TOB(tob) => tob.exact_in()
        }
    }

    fn max_gas_token_0(&self) -> u128 {
        match self {
            AllOrders::Standing(p) => p.max_gas_token_0(),
            AllOrders::Flash(kof) => kof.max_gas_token_0(),
            AllOrders::TOB(tob) => tob.max_gas_token_0()
        }
    }

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

    fn amount(&self) -> u128 {
        match self {
            AllOrders::Standing(p) => p.amount(),
            AllOrders::Flash(kof) => kof.amount(),
            AllOrders::TOB(tob) => tob.amount()
        }
    }

    fn limit_price(&self) -> U256 {
        match self {
            AllOrders::Standing(p) => p.limit_price(),
            AllOrders::Flash(kof) => kof.limit_price(),
            AllOrders::TOB(t) => t.limit_price()
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

    fn use_internal(&self) -> bool {
        match self {
            AllOrders::Standing(p) => p.use_internal(),
            AllOrders::Flash(kof) => kof.use_internal(),
            AllOrders::TOB(tob) => tob.use_internal()
        }
    }

    fn order_signature(&self) -> eyre::Result<PrimitiveSignature> {
        match self {
            AllOrders::Standing(p) => p.order_signature(),
            AllOrders::Flash(kof) => kof.order_signature(),
            AllOrders::TOB(tob) => tob.order_signature()
        }
    }
}

impl RawPoolOrder for GroupedVanillaOrder {
    fn exact_in(&self) -> bool {
        match self {
            GroupedVanillaOrder::Standing(p) => p.exact_in(),
            GroupedVanillaOrder::KillOrFill(kof) => kof.exact_in()
        }
    }

    fn min_amount(&self) -> u128 {
        match self {
            GroupedVanillaOrder::Standing(p) => p.min_amount(),
            GroupedVanillaOrder::KillOrFill(kof) => kof.min_amount()
        }
    }

    fn max_gas_token_0(&self) -> u128 {
        match self {
            GroupedVanillaOrder::Standing(p) => p.max_gas_token_0(),
            GroupedVanillaOrder::KillOrFill(kof) => kof.max_gas_token_0()
        }
    }

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

    fn amount(&self) -> u128 {
        match self {
            GroupedVanillaOrder::Standing(p) => p.amount(),
            GroupedVanillaOrder::KillOrFill(kof) => kof.amount()
        }
    }

    fn limit_price(&self) -> U256 {
        match self {
            GroupedVanillaOrder::Standing(p) => p.limit_price(),
            GroupedVanillaOrder::KillOrFill(p) => p.limit_price()
        }
    }

    fn order_location(&self) -> OrderLocation {
        match &self {
            GroupedVanillaOrder::Standing(_) => OrderLocation::Limit,
            GroupedVanillaOrder::KillOrFill(_) => OrderLocation::Limit
        }
    }

    fn use_internal(&self) -> bool {
        match self {
            GroupedVanillaOrder::Standing(p) => p.use_internal(),
            GroupedVanillaOrder::KillOrFill(kof) => kof.use_internal()
        }
    }

    fn order_signature(&self) -> eyre::Result<PrimitiveSignature> {
        match self {
            GroupedVanillaOrder::Standing(p) => p.order_signature(),
            GroupedVanillaOrder::KillOrFill(kof) => kof.order_signature()
        }
    }
}

impl RawPoolOrder for GroupedComposableOrder {
    fn min_amount(&self) -> u128 {
        match self {
            GroupedComposableOrder::Partial(p) => p.min_amount(),
            GroupedComposableOrder::KillOrFill(kof) => kof.min_amount()
        }
    }

    fn exact_in(&self) -> bool {
        match self {
            GroupedComposableOrder::Partial(p) => p.exact_in(),
            GroupedComposableOrder::KillOrFill(kof) => kof.exact_in()
        }
    }

    fn max_gas_token_0(&self) -> u128 {
        match self {
            GroupedComposableOrder::Partial(p) => p.max_gas_token_0(),
            GroupedComposableOrder::KillOrFill(kof) => kof.max_gas_token_0()
        }
    }

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

    fn amount(&self) -> u128 {
        match self {
            GroupedComposableOrder::Partial(p) => p.amount(),
            GroupedComposableOrder::KillOrFill(kof) => kof.amount()
        }
    }

    fn limit_price(&self) -> U256 {
        match self {
            GroupedComposableOrder::Partial(p) => p.limit_price(),
            GroupedComposableOrder::KillOrFill(p) => p.limit_price()
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

    fn use_internal(&self) -> bool {
        match self {
            GroupedComposableOrder::Partial(p) => p.use_internal(),
            GroupedComposableOrder::KillOrFill(kof) => kof.use_internal()
        }
    }

    fn order_signature(&self) -> eyre::Result<PrimitiveSignature> {
        match self {
            GroupedComposableOrder::Partial(p) => p.order_signature(),
            GroupedComposableOrder::KillOrFill(kof) => kof.order_signature()
        }
    }
}

#[cfg(test)]
mod tests {
    use alloy::primitives::Uint;
    use angstrom_types::{
        matching::{uniswap::PoolSnapshot, Ray, SqrtPriceX96},
        primitive::PoolId
    };
    use testing_tools::type_generator::{
        amm::generate_amm_with_liquidity, orders::UserOrderBuilder
    };

    #[test]
    fn test_fetch_supply_or_demand_contribution_with_fee() {
        // 0.0000000000001 rate
        let min_price = Ray::from(Uint::from(100_000_000_000_000u128));

        // exact in bid
        let bid_order = UserOrderBuilder::new()
            .exact()
            .amount(1_000_000)
            .exact_in(true)
            .bid_min_price(min_price)
            .with_storage()
            .bid()
            .build();

        // the amount of demand should be
        // 1_000_000  * 1e27 / min_price
        let demand = Ray::from(Uint::from(10000000000000000000u128))
            - Ray::from(bid_order.priority_data.gas);

        let got_demand = bid_order.fetch_supply_or_demand_contribution_with_fee(min_price, 0);
        assert_eq!(demand, got_demand);

        // exact out bid
        let bid_order = UserOrderBuilder::new()
            .exact()
            .amount(1_000_000)
            .exact_in(false)
            .bid_min_price(min_price)
            .with_storage()
            .bid()
            .build();

        let got_demand = bid_order.fetch_supply_or_demand_contribution_with_fee(min_price, 0);
        assert_eq!(got_demand, Ray::from(Uint::from(1_000_000)));

        let ask_order = UserOrderBuilder::new()
            .exact()
            .amount(1_000_000)
            .exact_in(true)
            .min_price(min_price)
            .with_storage()
            .ask()
            .build();

        let got_demand = ask_order.fetch_supply_or_demand_contribution_with_fee(min_price, 0);
        assert_eq!(got_demand, Ray::from(Uint::from(1_000_000)));

        let ask_order = UserOrderBuilder::new()
            .exact()
            .amount(1_000_000)
            .exact_in(false)
            .min_price(min_price)
            .with_storage()
            .ask()
            .build();

        let demand = Ray::from(Uint::from(10000000000000000000u128))
            + Ray::from(ask_order.priority_data.gas);
        let got_demand = ask_order.fetch_supply_or_demand_contribution_with_fee(min_price, 0);
        assert_eq!(got_demand, demand);
    }
}
