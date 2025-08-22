use std::{hash::Hash, ops::Deref};

use alloy::{
    primitives::{Address, FixedBytes, TxHash, U256},
    signers::Signature
};
use alloy_primitives::B256;
use pade::PadeDecode;
use serde::{Deserialize, Serialize};

use super::{GenerateFlippedOrder, RawPoolOrder, RespendAvoidanceMethod};
use crate::{
    matching::Ray,
    orders::{OrderId, OrderLocation, OrderPriorityData},
    primitive::{ANGSTROM_DOMAIN, PoolId, UserAccountVerificationError},
    sol_bindings::rpc_orders::{
        ExactFlashOrder, ExactStandingOrder, OmitOrderMeta, PartialFlashOrder,
        PartialStandingOrder, TopOfBlockOrder
    }
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum AllOrders {
    ExactStanding(ExactStandingOrder),
    PartialStanding(PartialStandingOrder),
    ExactFlash(ExactFlashOrder),
    PartialFlash(PartialFlashOrder),
    TOB(TopOfBlockOrder)
}

impl From<TopOfBlockOrder> for AllOrders {
    fn from(value: TopOfBlockOrder) -> Self {
        Self::TOB(value)
    }
}

impl AllOrders {
    pub fn order_hash(&self) -> FixedBytes<32> {
        match self {
            Self::ExactStanding(p) => p.unique_order_hash(p.from()),
            Self::PartialStanding(p) => p.unique_order_hash(p.from()),
            Self::ExactFlash(p) => p.unique_order_hash(p.from()),
            Self::PartialFlash(p) => p.unique_order_hash(p.from()),
            Self::TOB(t) => t.unique_order_hash(t.from())
        }
    }

    pub fn is_vanilla(&self) -> bool {
        !match self {
            Self::ExactStanding(p) => p.has_hook(),
            Self::PartialStanding(p) => p.has_hook(),
            Self::ExactFlash(p) => p.has_hook(),
            Self::PartialFlash(p) => p.has_hook(),
            Self::TOB(_) => false
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
    pub is_currently_valid: Option<UserAccountVerificationError>,
    /// what side of the book does this order lay on
    pub is_bid:             bool,
    /// is valid order
    pub is_valid:           bool,
    /// the block the order was validated for
    pub valid_block:        u64,
    /// holds expiry data
    pub order_id:           OrderId,
    pub tob_reward:         U256,
    pub cancel_requested:   bool
}

impl<O: RawPoolOrder> OrderWithStorageData<O> {
    pub fn is_currently_valid(&self) -> bool {
        self.is_currently_valid.is_none()
    }

    pub fn with_default(order: O) -> Self {
        Self {
            is_bid: order.is_bid(),
            order,
            priority_data: OrderPriorityData::default(),
            pool_id: Default::default(),
            invalidates: vec![],
            is_currently_valid: None,
            is_valid: true,
            valid_block: 0,
            order_id: OrderId::default(),
            tob_reward: U256::default(),
            cancel_requested: false
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
            AllOrders::ExactStanding(p) => p.from(),
            AllOrders::PartialStanding(p) => p.from(),
            AllOrders::ExactFlash(p) => p.from(),
            AllOrders::PartialFlash(p) => p.from(),
            AllOrders::TOB(t) => t.from()
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
            tob_reward:         self.tob_reward,
            cancel_requested:   self.cancel_requested
        })
    }
}

impl OrderWithStorageData<AllOrders> {
    pub fn pre_fee_and_gas_price(&self, fee: u128) -> Ray {
        let price =
            self.unscale_by_gas_fee(self.order.price(), self.priority_data.gas.to::<u128>());

        price.unscale_to_fee(fee)
    }
}

impl AllOrders {
    pub fn hash(&self) -> FixedBytes<32> {
        self.order_hash()
    }

    /// Primarily used for debugging to work with price as an f64
    pub fn float_price(&self) -> f64 {
        Ray::from(self.limit_price()).as_f64()
    }

    /// Bid orders need to invert their price
    pub fn bid_price(&self) -> Ray {
        self.price().inv_ray_round(true)
    }

    /// Get the appropriate price when passed a bool telling us if we're looking
    /// for a bid-side price or not
    ///
    /// TODO:  Deprecate this and replace with `price_t1_over_t0()` since that
    /// performs this function more elegantly
    pub fn price_for_book_side(&self, is_bid: bool) -> Ray {
        if is_bid { self.bid_price() } else { self.price() }
    }

    /// Provides the LITERAL price as specified in the order.  Note that for
    /// bids this can be inverse
    pub fn price(&self) -> Ray {
        self.limit_price().into()
    }

    /// Provides the price in T1/T0 format.  For "ask" orders this means just
    /// providing the literal price.  For "Bid" orders this means inverting the
    /// price to be in T1/T0 format since we store those prices as T0/T1
    pub fn price_t1_over_t0(&self) -> Ray {
        if self.is_bid() { self.price().inv_ray_round(true) } else { self.price() }
    }

    pub fn pre_fee_and_gas_price(&self, fee: u128, gas_t0: u128) -> Ray {
        let price = self.price().unscale_to_fee(fee);

        self.unscale_by_gas_fee(price, gas_t0)
    }

    /// Provides the fee-adjusted price to be used for accounting.  Note that
    /// for bids this can be inverse
    pub fn fee_adj_price(&self, fee: u128) -> Ray {
        self.price().scale_to_fee(fee)
    }
}

impl RawPoolOrder for TopOfBlockOrder {
    fn min_qty_t0(&self) -> Option<u128> {
        self.is_bid().then_some(self.quantity_out)
    }

    fn exact_in(&self) -> bool {
        true
    }

    fn is_tob(&self) -> bool {
        true
    }

    fn has_hook(&self) -> bool {
        false
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
        self.unique_order_hash(self.from())
    }

    // gas fee doesn't need scaling for tob.
    fn unscale_by_gas_fee(&self, price: Ray, _: u128) -> Ray {
        price
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
        U256::from(self.quantity_out) / U256::from(self.quantity_in)
    }

    fn token_in(&self) -> Address {
        self.asset_in
    }

    fn token_out(&self) -> Address {
        self.asset_out
    }

    fn is_valid_signature(&self) -> bool {
        let Ok(sig) = self.order_signature() else { return false };
        let hash = self.no_meta_eip712_signing_hash(ANGSTROM_DOMAIN.get().unwrap());

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

    fn order_signature(&self) -> eyre::Result<Signature> {
        let s = self.meta.signature.to_vec();
        let mut slice = s.as_slice();

        Ok(Signature::pade_decode(&mut slice, None)?)
    }
}

impl RawPoolOrder for PartialStandingOrder {
    fn unscale_by_gas_fee(&self, price: Ray, gas_amount_t0: u128) -> Ray {
        match self.is_bid() {
            true => {
                // if exact in bid
                // t1 -> t0
                // quantityOut = price.convertDown(quantityIn) - fee;
                // quantityIn = AmountIn.wrap(quantity);
                let amount_in = self.max_amount_in;
                let amount_out = price.quantity(amount_in, false) + gas_amount_t0;

                // We always round up to leave a buffer
                Ray::from_quantities(amount_out, amount_in, true)
            }
            false => {
                // exact in ask
                // t0 -> t1
                // quantityIn = AmountIn.wrap(quantity);
                // quantityOut = price.convertDown(quantityIn - fee);
                let amount_in = self.max_amount_in;
                let amount_out = price.quantity(amount_in + gas_amount_t0, false);

                Ray::from_quantities(amount_out, amount_in, true)
            }
        }
    }

    fn has_hook(&self) -> bool {
        !self.hook_data.is_empty()
    }

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
        let hash = self.no_meta_eip712_signing_hash(ANGSTROM_DOMAIN.get().unwrap());

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
        self.unique_order_hash(self.from())
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

    fn order_signature(&self) -> eyre::Result<Signature> {
        let s = self.meta.signature.to_vec();
        let mut slice = s.as_slice();

        Ok(Signature::pade_decode(&mut slice, None)?)
    }
}

impl RawPoolOrder for ExactStandingOrder {
    fn unscale_by_gas_fee(&self, price: Ray, gas_amount_t0: u128) -> Ray {
        match (self.exact_in, self.is_bid()) {
            (true, true) => {
                // if exact in bid
                // t1 -> t0
                // quantityOut = price.convertDown(quantityIn) - fee;
                // quantityIn = AmountIn.wrap(quantity);
                let amount_in = self.amount;
                let amount_out = price.quantity(amount_in, false) + gas_amount_t0;

                // We always round up to leave a buffer
                Ray::from_quantities(amount_out, amount_in, true)
            }
            (false, true) => {
                // exact out bid
                // t1 -> t0
                // quantityOut = AmountOut.wrap(quantity);
                // quantityIn = price.convertUp(quantityOut + fee);
                let amount_out = self.amount;
                let amount_in = price.inverse_quantity(amount_out - gas_amount_t0, true);

                Ray::from_quantities(amount_out, amount_in, true)
            }
            (true, false) => {
                // exact in ask
                // t0 -> t1
                // quantityIn = AmountIn.wrap(quantity);
                // quantityOut = price.convertDown(quantityIn - fee);
                let amount_in = self.amount;
                let amount_out = price.quantity(amount_in + gas_amount_t0, false);

                Ray::from_quantities(amount_out, amount_in, true)
            }
            (false, false) => {
                // exact out ask
                // t0 -> t1
                // quantityOut = AmountOut.wrap(quantity);
                // quantityIn = price.convertUp(quantityOut) + fee;
                let amount_out = self.amount;
                let amount_in = price.inverse_quantity(amount_out, true) - gas_amount_t0;

                Ray::from_quantities(amount_out, amount_in, true)
            }
        }
    }

    fn has_hook(&self) -> bool {
        !self.hook_data.is_empty()
    }

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
        let hash = self.no_meta_eip712_signing_hash(ANGSTROM_DOMAIN.get().unwrap());

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
        self.unique_order_hash(self.from())
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

    fn order_signature(&self) -> eyre::Result<Signature> {
        let s = self.meta.signature.to_vec();
        let mut slice = s.as_slice();

        Ok(Signature::pade_decode(&mut slice, None)?)
    }
}

impl RawPoolOrder for PartialFlashOrder {
    fn unscale_by_gas_fee(&self, price: Ray, gas_amount_t0: u128) -> Ray {
        match self.is_bid() {
            true => {
                // if exact in bid
                // t1 -> t0
                // quantityOut = price.convertDown(quantityIn) - fee;
                // quantityIn = AmountIn.wrap(quantity);
                let amount_in = self.max_amount_in;
                let amount_out = price.quantity(amount_in, false) + gas_amount_t0;

                // We always round up to leave a buffer
                Ray::from_quantities(amount_out, amount_in, true)
            }
            false => {
                // exact in ask
                // t0 -> t1
                // quantityIn = AmountIn.wrap(quantity);
                // quantityOut = price.convertDown(quantityIn - fee);
                let amount_in = self.max_amount_in;
                let amount_out = price.quantity(amount_in + gas_amount_t0, false);

                Ray::from_quantities(amount_out, amount_in, true)
            }
        }
    }

    fn has_hook(&self) -> bool {
        !self.hook_data.is_empty()
    }

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
        let hash = self.no_meta_eip712_signing_hash(ANGSTROM_DOMAIN.get().unwrap());

        sig.recover_address_from_prehash(&hash)
            .map(|addr| addr == self.meta.from)
            .unwrap_or_default()
    }

    fn flash_block(&self) -> Option<u64> {
        Some(self.valid_for_block)
    }

    fn order_hash(&self) -> TxHash {
        self.unique_order_hash(self.from())
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

    fn order_signature(&self) -> eyre::Result<Signature> {
        let s = self.meta.signature.to_vec();
        let mut slice = s.as_slice();

        Ok(Signature::pade_decode(&mut slice, None)?)
    }
}

impl RawPoolOrder for ExactFlashOrder {
    fn unscale_by_gas_fee(&self, price: Ray, gas_amount_t0: u128) -> Ray {
        match (self.exact_in, self.is_bid()) {
            (true, true) => {
                // if exact in bid
                // t1 -> t0
                // quantityOut = price.convertDown(quantityIn) - fee;
                // quantityIn = AmountIn.wrap(quantity);
                let amount_in = self.amount;
                let amount_out = price.quantity(amount_in, false) + gas_amount_t0;

                // We always round up to leave a buffer
                Ray::from_quantities(amount_out, amount_in, true)
            }
            (false, true) => {
                // exact out bid
                // t1 -> t0
                // quantityOut = AmountOut.wrap(quantity);
                // quantityIn = price.convertUp(quantityOut + fee);
                let amount_out = self.amount;
                let amount_in = price.inverse_quantity(amount_out - gas_amount_t0, true);

                Ray::from_quantities(amount_out, amount_in, true)
            }
            (true, false) => {
                // exact in ask
                // t0 -> t1
                // quantityIn = AmountIn.wrap(quantity);
                // quantityOut = price.convertDown(quantityIn - fee);
                let amount_in = self.amount;
                let amount_out = price.quantity(amount_in + gas_amount_t0, false);

                Ray::from_quantities(amount_out, amount_in, true)
            }
            (false, false) => {
                // exact out ask
                // t0 -> t1
                // quantityOut = AmountOut.wrap(quantity);
                // quantityIn = price.convertUp(quantityOut) + fee;
                let amount_out = self.amount;
                let amount_in = price.inverse_quantity(amount_out, true) - gas_amount_t0;

                Ray::from_quantities(amount_out, amount_in, true)
            }
        }
    }

    fn has_hook(&self) -> bool {
        !self.hook_data.is_empty()
    }

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
        let hash = self.no_meta_eip712_signing_hash(ANGSTROM_DOMAIN.get().unwrap());

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
        self.unique_order_hash(self.from())
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

    fn order_signature(&self) -> eyre::Result<Signature> {
        let s = self.meta.signature.to_vec();
        let mut slice = s.as_slice();

        Ok(Signature::pade_decode(&mut slice, None)?)
    }
}

impl RawPoolOrder for AllOrders {
    fn is_tob(&self) -> bool {
        matches!(self, AllOrders::TOB(_))
    }

    fn unscale_by_gas_fee(&self, price: Ray, gas_amount_t0: u128) -> Ray {
        match self {
            AllOrders::ExactStanding(p) => p.unscale_by_gas_fee(price, gas_amount_t0),
            AllOrders::PartialStanding(p) => p.unscale_by_gas_fee(price, gas_amount_t0),
            AllOrders::ExactFlash(p) => p.unscale_by_gas_fee(price, gas_amount_t0),
            AllOrders::PartialFlash(p) => p.unscale_by_gas_fee(price, gas_amount_t0),
            AllOrders::TOB(t) => t.unscale_by_gas_fee(price, gas_amount_t0)
        }
    }

    fn has_hook(&self) -> bool {
        match self {
            AllOrders::ExactStanding(p) => p.has_hook(),
            AllOrders::PartialStanding(p) => p.has_hook(),
            AllOrders::ExactFlash(p) => p.has_hook(),
            AllOrders::PartialFlash(p) => p.has_hook(),
            AllOrders::TOB(t) => t.has_hook()
        }
    }

    fn min_amount(&self) -> u128 {
        match self {
            AllOrders::ExactStanding(p) => p.min_amount(),
            AllOrders::PartialStanding(p) => p.min_amount(),
            AllOrders::ExactFlash(p) => p.min_amount(),
            AllOrders::PartialFlash(p) => p.min_amount(),
            AllOrders::TOB(t) => t.min_amount()
        }
    }

    fn exact_in(&self) -> bool {
        match self {
            AllOrders::ExactStanding(p) => p.exact_in(),
            AllOrders::PartialStanding(p) => p.exact_in(),
            AllOrders::ExactFlash(p) => p.exact_in(),
            AllOrders::PartialFlash(p) => p.exact_in(),
            AllOrders::TOB(t) => t.exact_in()
        }
    }

    fn max_gas_token_0(&self) -> u128 {
        match self {
            AllOrders::ExactStanding(p) => p.max_gas_token_0(),
            AllOrders::PartialStanding(p) => p.max_gas_token_0(),
            AllOrders::ExactFlash(p) => p.max_gas_token_0(),
            AllOrders::PartialFlash(p) => p.max_gas_token_0(),
            AllOrders::TOB(t) => t.max_gas_token_0()
        }
    }

    fn is_valid_signature(&self) -> bool {
        match self {
            AllOrders::ExactStanding(p) => p.is_valid_signature(),
            AllOrders::PartialStanding(p) => p.is_valid_signature(),
            AllOrders::ExactFlash(p) => p.is_valid_signature(),
            AllOrders::PartialFlash(p) => p.is_valid_signature(),
            AllOrders::TOB(t) => t.is_valid_signature()
        }
    }

    fn from(&self) -> Address {
        match self {
            AllOrders::ExactStanding(p) => p.from(),
            AllOrders::PartialStanding(p) => p.from(),
            AllOrders::ExactFlash(p) => p.from(),
            AllOrders::PartialFlash(p) => p.from(),
            AllOrders::TOB(t) => t.from()
        }
    }

    fn order_hash(&self) -> TxHash {
        match self {
            AllOrders::ExactStanding(p) => p.order_hash(),
            AllOrders::PartialStanding(p) => p.order_hash(),
            AllOrders::ExactFlash(p) => p.order_hash(),
            AllOrders::PartialFlash(p) => p.order_hash(),
            AllOrders::TOB(t) => t.order_hash()
        }
    }

    fn respend_avoidance_strategy(&self) -> RespendAvoidanceMethod {
        match self {
            AllOrders::ExactStanding(p) => p.respend_avoidance_strategy(),
            AllOrders::PartialStanding(p) => p.respend_avoidance_strategy(),
            AllOrders::ExactFlash(p) => p.respend_avoidance_strategy(),
            AllOrders::PartialFlash(p) => p.respend_avoidance_strategy(),
            AllOrders::TOB(t) => t.respend_avoidance_strategy()
        }
    }

    fn deadline(&self) -> Option<U256> {
        match self {
            AllOrders::ExactStanding(p) => p.deadline(),
            AllOrders::PartialStanding(p) => p.deadline(),
            AllOrders::ExactFlash(p) => p.deadline(),
            AllOrders::PartialFlash(p) => p.deadline(),
            AllOrders::TOB(t) => t.deadline()
        }
    }

    fn amount(&self) -> u128 {
        match self {
            AllOrders::ExactStanding(p) => p.amount(),
            AllOrders::PartialStanding(p) => p.amount(),
            AllOrders::ExactFlash(p) => p.amount(),
            AllOrders::PartialFlash(p) => p.amount(),
            AllOrders::TOB(t) => t.amount()
        }
    }

    fn limit_price(&self) -> U256 {
        match self {
            AllOrders::ExactStanding(p) => p.limit_price(),
            AllOrders::PartialStanding(p) => p.limit_price(),
            AllOrders::ExactFlash(p) => p.limit_price(),
            AllOrders::PartialFlash(p) => p.limit_price(),
            AllOrders::TOB(t) => t.limit_price()
        }
    }

    fn token_out(&self) -> Address {
        match self {
            AllOrders::ExactStanding(p) => p.token_out(),
            AllOrders::PartialStanding(p) => p.token_out(),
            AllOrders::ExactFlash(p) => p.token_out(),
            AllOrders::PartialFlash(p) => p.token_out(),
            AllOrders::TOB(t) => t.token_out()
        }
    }

    fn token_in(&self) -> Address {
        match self {
            AllOrders::ExactStanding(p) => p.token_in(),
            AllOrders::PartialStanding(p) => p.token_in(),
            AllOrders::ExactFlash(p) => p.token_in(),
            AllOrders::PartialFlash(p) => p.token_in(),
            AllOrders::TOB(t) => t.token_in()
        }
    }

    fn flash_block(&self) -> Option<u64> {
        match self {
            AllOrders::ExactStanding(p) => p.flash_block(),
            AllOrders::PartialStanding(p) => p.flash_block(),
            AllOrders::ExactFlash(p) => p.flash_block(),
            AllOrders::PartialFlash(p) => p.flash_block(),
            AllOrders::TOB(t) => t.flash_block()
        }
    }

    fn order_location(&self) -> OrderLocation {
        match &self {
            AllOrders::ExactStanding(p) => p.order_location(),
            AllOrders::PartialStanding(p) => p.order_location(),
            AllOrders::ExactFlash(p) => p.order_location(),
            AllOrders::PartialFlash(p) => p.order_location(),
            AllOrders::TOB(t) => t.order_location()
        }
    }

    fn use_internal(&self) -> bool {
        match self {
            AllOrders::ExactStanding(p) => p.use_internal(),
            AllOrders::PartialStanding(p) => p.use_internal(),
            AllOrders::ExactFlash(p) => p.use_internal(),
            AllOrders::PartialFlash(p) => p.use_internal(),
            AllOrders::TOB(t) => t.use_internal()
        }
    }

    fn order_signature(&self) -> eyre::Result<Signature> {
        match self {
            AllOrders::ExactStanding(p) => p.order_signature(),
            AllOrders::PartialStanding(p) => p.order_signature(),
            AllOrders::ExactFlash(p) => p.order_signature(),
            AllOrders::PartialFlash(p) => p.order_signature(),
            AllOrders::TOB(t) => t.order_signature()
        }
    }
}
