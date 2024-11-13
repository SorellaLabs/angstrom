use std::{fmt::Debug, future::Future, pin::Pin};

use alloy::primitives::{Address, B256, U256};
use angstrom_types::{
    orders::OrderOrigin,
    sol_bindings::{
        ext::RawPoolOrder,
        grouped_orders::{
            AllOrders, GroupedComposableOrder, GroupedVanillaOrder, OrderWithStorageData
        },
        rpc_orders::TopOfBlockOrder
    }
};
use sim::SimValidation;
use state::token_pricing::TokenPriceGenerator;
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

pub type GasEstimationFuture<'a> =
    Pin<Box<dyn Future<Output = Result<u64, String>> + Send + Sync + 'a>>;

pub enum OrderValidationRequest {
    ValidateOrder(Sender<OrderValidationResults>, AllOrders, OrderOrigin)
}

/// TODO: not a fan of all the conversions. can def simplify
impl From<OrderValidationRequest> for OrderValidation {
    fn from(value: OrderValidationRequest) -> Self {
        match value {
            OrderValidationRequest::ValidateOrder(tx, order, orign) => match order {
                AllOrders::Standing(p) => {
                    // TODO: check hook data and deal with composable
                    // if p.hook_data.is_empty() {
                    OrderValidation::Limit(tx, GroupedVanillaOrder::Standing(p), orign)
                    // } else {
                    //
                    //     OrderValidation::LimitComposable(
                    //         tx,
                    //         GroupedComposableOrder::Partial(p),
                    //         orign
                    //     )
                    // }
                }
                AllOrders::Flash(kof) => {
                    // TODO: check hook data and deal with composable
                    // if kof.hook_data.is_empty() {
                    OrderValidation::Limit(tx, GroupedVanillaOrder::KillOrFill(kof), orign)
                    // } else {
                    //     OrderValidation::LimitComposable(
                    //         tx,
                    //         GroupedComposableOrder::KillOrFill(kof),
                    //         orign
                    //     )
                    // }
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

impl OrderValidationResults {
    pub fn add_gas_cost_or_invalidate<DB>(
        &mut self,
        sim: &SimValidation<DB>,
        token_price: &TokenPriceGenerator,
        is_limit: bool
    ) where
        DB: Unpin
            + Clone
            + 'static
            + revm::DatabaseRef
            + reth_provider::BlockNumReader
            + Send
            + Sync,
        <DB as revm::DatabaseRef>::Error: Send + Sync
    {
        // TODO: this can be done without a clone but is super annoying
        let this = self.clone();
        if let Self::Valid(order) = this {
            let order_hash = order.order_hash();
            let finalized_order = if is_limit {
                let res = Self::map_and_process(
                    order,
                    sim,
                    token_price,
                    |order| match order {
                        AllOrders::Standing(s) => GroupedVanillaOrder::Standing(s),
                        AllOrders::Flash(f) => GroupedVanillaOrder::KillOrFill(f),
                        _ => unreachable!()
                    },
                    |order| match order {
                        GroupedVanillaOrder::Standing(s) => AllOrders::Standing(s),
                        GroupedVanillaOrder::KillOrFill(s) => AllOrders::Flash(s)
                    },
                    SimValidation::calculate_user_gas
                );

                if res.is_err() {
                    *self = OrderValidationResults::Invalid(order_hash);

                    return
                }

                res
            } else {
                let res = Self::map_and_process(
                    order,
                    sim,
                    token_price,
                    |order| match order {
                        AllOrders::TOB(s) => s,
                        _ => unreachable!()
                    },
                    AllOrders::TOB,
                    SimValidation::calculate_tob_gas
                );
                if res.is_err() {
                    *self = OrderValidationResults::Invalid(order_hash);

                    return
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
        map_new: impl Fn(Old) -> New,
        map_old: impl Fn(New) -> Old,
        calculate_function: impl Fn(
            &SimValidation<DB>,
            &OrderWithStorageData<New>,
            &TokenPriceGenerator
        ) -> eyre::Result<(u64, U256)>
    ) -> eyre::Result<OrderWithStorageData<Old>>
    where
        DB: Unpin + Clone + 'static + revm::DatabaseRef + Send + Sync,
        <DB as revm::DatabaseRef>::Error: Sync + Send + 'static
    {
        let mut order = order
            .try_map_inner(move |order| Ok(map_new(order)))
            .unwrap();

        if let Ok((gas_units, gas_used)) = (calculate_function)(sim, &order, token_price) {
            order.priority_data.gas += gas_used;
            order.priority_data.gas_units = gas_units;
        } else {
            return Err(eyre::eyre!("not able to process gas"))
        }

        order.try_map_inner(move |new_order| Ok(map_old(new_order)))
    }
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

    /// estimates gas usage for order
    fn estimate_gas(&self, order: AllOrders) -> GasEstimationFuture;
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

    fn estimate_gas(&self, order: AllOrders) -> GasEstimationFuture {
        Box::pin(async move {
            match self.validate_order(OrderOrigin::External, order).await {
                OrderValidationResults::Valid(o) => Ok(o.priority_data.gas_units),
                OrderValidationResults::Invalid(e) => Err(format!("Invalid order: {}", e)),
                OrderValidationResults::TransitionedToBlock => {
                    Err("Order transitioned to block".to_string())
                }
            }
        })
    }
}
