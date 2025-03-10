use std::{fmt::Debug, sync::Arc};

use alloy::primitives::Address;
use angstrom_metrics::validation::ValidationMetrics;
use angstrom_types::sol_bindings::{
    RawPoolOrder,
    grouped_orders::{GroupedVanillaOrder, OrderWithStorageData},
    rpc_orders::TopOfBlockOrder
};
use gas::OrderGasCalculations;
use revm::primitives::ruint::aliases::U256;
use tracing::error_span;

use crate::{common::TokenPriceGenerator, order::sim::gas_inspector::GasUsed};

pub mod console_log;
mod gas;

// needed for future use
#[allow(unused)]
mod gas_inspector;

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

    pub fn calculate_tob_gas(
        &self,
        order: &OrderWithStorageData<TopOfBlockOrder>,
        conversion: &TokenPriceGenerator,
        block: u64
    ) -> eyre::Result<(GasUsed, GasInToken0)> {
        let hash = order.order_hash();
        let user = order.from();
        let span = error_span!("tob", ?hash, ?user);
        span.in_scope(|| {
            self.metrics.fetch_gas_for_user(true, || {
                let gas_in_wei = self.gas_calculator.gas_of_tob_order(order, block)?;
                // grab order tokens;
                let (token0, token1) = if order.asset_in < order.asset_out {
                    (order.asset_in, order.asset_out)
                } else {
                    (order.asset_out, order.asset_in)
                };

                // grab price conversion
                let conversion_factor =
                    conversion.get_eth_conversion_price(token0, token1).unwrap();

                Ok((gas_in_wei, (conversion_factor * U256::from(gas_in_wei)).scale_out_of_ray()))
            })
        })
    }

    pub fn calculate_user_gas(
        &self,
        order: &OrderWithStorageData<GroupedVanillaOrder>,
        conversion: &TokenPriceGenerator,
        block: u64
    ) -> eyre::Result<(GasUsed, GasInToken0)> {
        let hash = order.order_hash();
        let user = order.from();
        let span = error_span!("user", ?hash, ?user);
        span.in_scope(|| {
            self.metrics.fetch_gas_for_user(false, || {
                let gas_in_wei = self.gas_calculator.gas_of_book_order(order, block)?;
                // grab order tokens;
                let (token0, token1) = if order.token_in() < order.token_out() {
                    (order.token_in(), order.token_out())
                } else {
                    (order.token_out(), order.token_in())
                };

                // grab price conversion
                let conversion_factor =
                    conversion.get_eth_conversion_price(token0, token1).unwrap();

                Ok((gas_in_wei, (conversion_factor * U256::from(gas_in_wei)).scale_out_of_ray()))
            })
        })
    }
}
