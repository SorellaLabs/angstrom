use std::{fmt::Debug, future::Future, pin::Pin};

use alloy::primitives::{Address, B256, U256};
use angstrom_types::{
    orders::{OrderOrigin, UpdatedGas},
    primitive::OrderValidationError,
    sol_bindings::{
        ext::RawPoolOrder,
        grouped_orders::{AllOrders, GroupedComposableOrder, OrderWithStorageData}
    }
};
use sim::SimValidation;
use tokio::sync::oneshot::{Sender, channel};

use crate::{common::TokenPriceGenerator, validator::ValidationRequest};

pub mod order_validator;
pub mod sim;
pub mod state;

use crate::validator::ValidationClient;

pub type ValidationFuture<'a> =
    Pin<Box<dyn Future<Output = OrderValidationResults> + Send + Sync + 'a>>;

pub type ValidationsFuture<'a> =
    Pin<Box<dyn Future<Output = Vec<OrderValidationResults>> + Send + Sync + 'a>>;

pub type GasEstimationFuture<'a> =
    Pin<Box<dyn Future<Output = Result<U256, String>> + Send + Sync + 'a>>;

pub type NonceFuture<'a> = Pin<Box<dyn Future<Output = u64> + Send + Sync + 'a>>;

pub enum OrderValidationRequest {
    ValidateOrder(Sender<OrderValidationResults>, AllOrders, OrderOrigin)
}

/// TODO: not a fan of all the conversions. can def simplify
impl From<OrderValidationRequest> for OrderValidation {
    fn from(value: OrderValidationRequest) -> Self {
        match value {
            OrderValidationRequest::ValidateOrder(tx, order, orign) => match order {
                order @ AllOrders::PartialStanding(_) => OrderValidation::Limit(tx, order, orign),
                order @ AllOrders::ExactStanding(_) => OrderValidation::Limit(tx, order, orign),
                order @ AllOrders::ExactFlash(_) => OrderValidation::Limit(tx, order, orign),
                order @ AllOrders::PartialFlash(_) => OrderValidation::Limit(tx, order, orign),
                tob @ AllOrders::TOB(_) => OrderValidation::Searcher(tx, tob, orign)
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
    Invalid { hash: B256, error: OrderValidationError },
    TransitionedToBlock(Vec<UpdatedGas>)
}

impl OrderValidationResults {
    pub fn add_gas_cost_or_invalidate<DB>(
        &mut self,
        sim: &SimValidation<DB>,
        token_price: &TokenPriceGenerator,
        is_limit: bool,
        block: u64
    ) where
        DB: Unpin
            + Clone
            + 'static
            + revm::DatabaseRef
            + reth_provider::BlockNumReader
            + Send
            + Sync,
        <DB as revm::DatabaseRef>::Error: Send + Sync + std::fmt::Debug
    {
        let this = self.clone();
        if let Self::Valid(order) = this {
            let order_hash = order.order_hash();
            let finalized_order = if is_limit {
                let res = Self::map_and_process(
                    order,
                    sim,
                    token_price,
                    block,
                    |order| order,
                    |order| order,
                    SimValidation::calculate_user_gas
                );

                if let Err(e) = res {
                    tracing::info!(%e, "failed to add gas to order");
                    *self = OrderValidationResults::Invalid {
                        hash:  order_hash,
                        error: OrderValidationError::NotEnoughGas
                    };

                    return;
                }

                res
            } else {
                let res = Self::map_and_process(
                    order,
                    sim,
                    token_price,
                    block,
                    |order| match order {
                        AllOrders::TOB(s) => s,
                        _ => unreachable!()
                    },
                    AllOrders::TOB,
                    SimValidation::calculate_tob_gas
                );
                if let Err(e) = res {
                    tracing::info!(%e, "failed to add gas to order");
                    *self = OrderValidationResults::Invalid {
                        hash:  order_hash,
                        error: OrderValidationError::NotEnoughGas
                    };

                    return;
                }

                res
            };

            *self = OrderValidationResults::Valid(finalized_order.unwrap())
        }
    }

    // hmm the structure here is probably overkill to avoid 8 extra lines of code
    fn map_and_process<Old, New, DB>(
        order: OrderWithStorageData<Old>,
        sim: &SimValidation<DB>,
        token_price: &TokenPriceGenerator,
        block: u64,
        map_new: impl Fn(Old) -> New,
        map_old: impl Fn(New) -> Old,
        calculate_function: impl Fn(
            &SimValidation<DB>,
            &OrderWithStorageData<New>,
            &TokenPriceGenerator,
            u64
        ) -> eyre::Result<(u64, U256)>
    ) -> eyre::Result<OrderWithStorageData<Old>>
    where
        DB: Unpin + Clone + 'static + revm::DatabaseRef + Send + Sync,
        <DB as revm::DatabaseRef>::Error: Sync + Send + 'static
    {
        let mut order = order
            .try_map_inner(move |order| Ok(map_new(order)))
            .unwrap();

        let (gas_units, gas_used) = (calculate_function)(sim, &order, token_price, block)?;
        // ensure that gas used is less than the max gas specified

        order.priority_data.gas += gas_used;
        order.priority_data.gas_units = gas_units;

        order.try_map_inner(move |new_order| Ok(map_old(new_order)))
    }
}

pub enum OrderValidation {
    Limit(Sender<OrderValidationResults>, AllOrders, OrderOrigin),
    LimitComposable(Sender<OrderValidationResults>, GroupedComposableOrder, OrderOrigin),
    Searcher(Sender<OrderValidationResults>, AllOrders, OrderOrigin)
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

    fn cancel_order(&self, user: Address, order_hash: B256);

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

    /// estimates gas usage for order
    fn estimate_gas(
        &self,
        is_book: bool,
        is_internal: bool,
        token_0: Address,
        token_1: Address
    ) -> GasEstimationFuture;

    fn valid_nonce_for_user(&self, address: Address) -> NonceFuture;
}

impl OrderValidatorHandle for ValidationClient {
    type Order = AllOrders;

    fn cancel_order(&self, user: Address, order_hash: B256) {
        let _ = self
            .0
            .send(ValidationRequest::CancelOrder { user, order_hash });
    }

    fn new_block(
        &self,
        block_number: u64,
        orders: Vec<B256>,
        addresses: Vec<Address>
    ) -> ValidationFuture {
        Box::pin(async move {
            let (tx, rx) = channel();
            let _ = self.0.send(ValidationRequest::NewBlock {
                sender: tx,
                block_number,
                orders,
                addresses
            });

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

    fn estimate_gas(
        &self,
        is_book: bool,
        is_internal: bool,
        token_0: Address,
        token_1: Address
    ) -> GasEstimationFuture {
        Box::pin(async move {
            let (sender, rx) = channel();
            let _ = self.0.send(ValidationRequest::GasEstimation {
                sender,
                is_book,
                is_internal,
                token_0,
                token_1
            });

            rx.await.unwrap().map_err(|e| e.to_string())
        })
    }

    fn valid_nonce_for_user(&self, address: Address) -> NonceFuture {
        Box::pin(async move {
            let (tx, rx) = channel();
            let _ = self
                .0
                .send(ValidationRequest::Nonce { sender: tx, user_address: address });

            rx.await.unwrap()
        })
    }
}
