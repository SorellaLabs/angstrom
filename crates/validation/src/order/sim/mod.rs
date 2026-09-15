use std::{fmt::Debug, sync::Arc};

use alloy::primitives::Address;
use angstrom_metrics::validation::ValidationMetrics;
use angstrom_types::{
    primitive::UserAccountVerificationError,
    sol_bindings::{
        RawPoolOrder,
        grouped_orders::{AllOrders, OrderWithStorageData},
        rpc_orders::TopOfBlockOrder
    }
};
use gas::OrderGasCalculations;
use revm::primitives::ruint::aliases::U256;
use tracing::error_span;

use crate::common::TokenPriceGenerator;

pub mod console_log;
mod gas;
pub use gas::{
    BOOK_GAS, BOOK_GAS_INTERNAL, SWITCH_WEI, TOB_GAS_INTERNAL_NORMAL, TOB_GAS_INTERNAL_SUB,
    TOB_GAS_NORMAL, TOB_GAS_SUB
};

pub type GasUsed = u64;
// needed for future use
// mod gas_inspector;
//
pub type GasReturn = (Option<(GasUsed, GasInToken0)>, Option<UserAccountVerificationError>);

pub type GasInToken0 = U256;
/// validation relating to simulations.
#[derive(Clone)]
pub struct SimValidation<DB> {
    gas_calculator: OrderGasCalculations<DB>,
    metrics:        ValidationMetrics
}

impl<DB> SimValidation<DB>
where
    DB: Unpin + Clone + 'static + revm::DatabaseRef + reth_provider::BlockNumReader + Send + Sync,
    <DB as revm::DatabaseRef>::Error: Send + Sync + Debug
{
    pub fn new(db: Arc<DB>, angstrom_address: Address, node_address: Address) -> Self {
        let gas_calculator =
            OrderGasCalculations::new(db.clone(), Some(angstrom_address), node_address)
                .expect("failed to deploy baseline angstrom for gas calculations");
        Self { gas_calculator, metrics: ValidationMetrics::new() }
    }

    /// The same validation reading through `db`.
    pub fn repoint(&self, db: Arc<DB>) -> Self {
        Self {
            gas_calculator: self.gas_calculator.repoint(db),
            metrics:        self.metrics.clone()
        }
    }

    /// returns an error if we fail to convert prices or if the amount of token
    /// zero for gas is greater than the max amount specified.
    pub fn calculate_tob_gas(
        &self,
        order: &OrderWithStorageData<TopOfBlockOrder>,
        conversion: &TokenPriceGenerator,
        block: u64
    ) -> eyre::Result<GasReturn> {
        let hash = order.order_hash();
        let user = order.from();
        let span = error_span!("tob", ?hash, ?user);
        span.in_scope(|| {
            self.metrics.fetch_gas_for_user(true, || {
                let wei = conversion.base_wei;
                let gas_in_wei = self.gas_calculator.gas_of_tob_order(order, block, wei)?;
                // grab order tokens;
                let (token0, token1, max_gas) = if order.asset_in < order.asset_out {
                    (order.asset_in, order.asset_out, order.max_gas_token_0())
                } else {
                    (order.asset_out, order.asset_in, order.max_gas_token_0())
                };

                let gas_token_0 = conversion
                    .get_eth_conversion_price(token0, token1, gas_in_wei)
                    .ok_or_else(|| eyre::eyre!("failed to get conversion price"))?;

                // For TOB orders, given they are only valid for the current block,
                // we return a error given the order will never be included.
                if gas_token_0 > max_gas {
                    return Err(eyre::eyre!("tob order doesn't have enough gas for target block"));
                }

                Ok((Some((gas_in_wei, U256::from(gas_token_0))), None))
            })
        })
    }

    /// returns an error if we fail to convert prices or if the amount of token
    /// zero for gas is greater than the max amount specified.
    pub fn calculate_user_gas(
        &self,
        order: &OrderWithStorageData<AllOrders>,
        conversion: &TokenPriceGenerator,
        block: u64
    ) -> eyre::Result<GasReturn> {
        let hash = order.order_hash();
        let user = order.from();
        let span = error_span!("user", ?hash, ?user);
        span.in_scope(|| {
            self.metrics.fetch_gas_for_user(false, || {
                let gas_in_wei = self.gas_calculator.gas_of_book_order(order, block)?;
                // grab order tokens;
                let (token0, token1, max_gas) = if order.token_in() < order.token_out() {
                    (order.token_in(), order.token_out(), order.max_gas_token_0())
                } else {
                    (order.token_out(), order.token_in(), order.max_gas_token_0())
                };

                let gas_token_0 = conversion
                    .get_eth_conversion_price(token0, token1, gas_in_wei)
                    .ok_or_else(|| eyre::eyre!("failed to get conversion price"))?;

                // convert to u256 for overflow cases.
                if gas_token_0 > max_gas {
                    let err = UserAccountVerificationError::NotEnoughGas {
                        needed_gas: gas_token_0,
                        set_gas:    max_gas
                    };
                    return Ok((Some((gas_in_wei, U256::from(gas_token_0))), Some(err)));
                }

                Ok((Some((gas_in_wei, U256::from(gas_token_0))), None))
            })
        })
    }
}
