use std::{fmt::Debug, future::Future, pin::Pin};

use alloy_primitives::Address;
use angstrom_types::{
    orders::{OrderId, OrderOrigin},
    sol_bindings::{
        grouped_orders::{
            AllOrders, GroupedComposableOrder, GroupedVanillaOrder, OrderWithStorageData,
            RawPoolOrder
        },
        sol::TopOfBlockOrder
    }
};
use reth_primitives::B256;
use state::account::user::UserAddress;
use tokio::sync::oneshot::{channel, Sender};

use crate::validator::ValidationRequest;

pub mod order_validator;
pub mod sim;
pub mod state;

use crate::validator::ValidationClient;

pub type ValidationFuture<'a> =
    Pin<Box<dyn Future<Output = OrderValidationResults> + Send + Sync + 'a>>;

pub type ValidationsFuture<'a> =
    Pin<Box<dyn Future<Output = Vec<OrderValidationResults>> + Send + Sync + 'a>>;

pub enum OrderValidationRequest {
    ValidateOrder(Sender<OrderValidationResults>, AllOrders, OrderOrigin)
}

/// TODO: not a fan of all the conversions. can def simplify
impl From<OrderValidationRequest> for OrderValidation {
    fn from(value: OrderValidationRequest) -> Self {
        match value {
            OrderValidationRequest::ValidateOrder(tx, order, orign) => match order {
                AllOrders::Partial(p) => {
                    if p.hook_data.is_empty() {
                        OrderValidation::Limit(tx, GroupedVanillaOrder::Partial(p), orign)
                    } else {
                        OrderValidation::LimitComposable(
                            tx,
                            GroupedComposableOrder::Partial(p),
                            orign
                        )
                    }
                }
                AllOrders::KillOrFill(kof) => {
                    if kof.hook_data.is_empty() {
                        OrderValidation::Limit(tx, GroupedVanillaOrder::KillOrFill(kof), orign)
                    } else {
                        OrderValidation::LimitComposable(
                            tx,
                            GroupedComposableOrder::KillOrFill(kof),
                            orign
                        )
                    }
                }
                AllOrders::TOB(tob) => OrderValidation::Searcher(tx, tob, orign)
            }
        }
    }
}

pub enum ValidationMessage {
    ValidationResults(OrderValidationResults)
}

#[derive(Debug, Clone)]
pub enum OrderValidationResults {
    Valid(OrderWithStorageData<AllOrders>),
    // the raw hash to be removed
    Invalid(B256),
    TransitionedToBlock
}

pub enum OrderValidation {
    Limit(Sender<OrderValidationResults>, GroupedVanillaOrder, OrderOrigin),
    LimitComposable(Sender<OrderValidationResults>, GroupedComposableOrder, OrderOrigin),
    Searcher(Sender<OrderValidationResults>, TopOfBlockOrder, OrderOrigin)
}
impl OrderValidation {
    pub fn user(&self) -> Address {
        match &self {
            Self::Searcher(_, u, _) => u.from(),
            Self::LimitComposable(_, u, _) => u.from(),
            Self::Limit(_, u, _) => u.from()
        }
    }
}

/// Provides support for validating transaction at any given state of the chain
pub trait OrderValidatorHandle: Send + Sync + Clone + Debug + Unpin + 'static {
    /// The order type of the limit order pool
    type Order: Send + Sync;

    fn validate_order(&self, origin: OrderOrigin, transaction: Self::Order) -> ValidationFuture;

    /// Validates a batch of orders.
    ///
    /// Must return all outcomes for the given orders in the same order.
    fn validate_orders(&self, transactions: Vec<(OrderOrigin, Self::Order)>) -> ValidationsFuture {
        Box::pin(futures_util::future::join_all(
            transactions
                .into_iter()
                .map(|(origin, tx)| self.validate_order(origin, tx))
        ))
    }

    /// orders that are either expired or have been filled.
    fn new_block(
        &self,
        block_number: u64,
        completed_orders: Vec<B256>,
        addresses: Vec<Address>
    ) -> ValidationFuture;
}

impl OrderValidatorHandle for ValidationClient {
    type Order = AllOrders;

    fn new_block(
        &self,
        block_number: u64,
        orders: Vec<B256>,
        addresses: Vec<Address>
    ) -> ValidationFuture {
        Box::pin(async move {
            let (tx, rx) = channel();
            let _ = self
                .0
                .send(ValidationRequest::NewBlock { block_number, orders, addresses });

            rx.await.unwrap()
        })
    }

    fn validate_order(&self, origin: OrderOrigin, transaction: Self::Order) -> ValidationFuture {
        Box::pin(async move {
            let (tx, rx) = channel();
            let _ = self
                .0
                .send(ValidationRequest::Order(OrderValidationRequest::ValidateOrder(
                    tx,
                    transaction,
                    origin
                )));

            rx.await.unwrap()
        })
    }
}
