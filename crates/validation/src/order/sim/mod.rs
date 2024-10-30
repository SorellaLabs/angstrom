use std::sync::Arc;

use alloy::primitives::Address;
use angstrom_types::sol_bindings::{
    grouped_orders::{GroupedVanillaOrder, OrderWithStorageData},
    rpc_orders::TopOfBlockOrder
};
use gas::OrderGasCalculations;
use gas_inspector::GasUsed;
use revm::primitives::ruint::aliases::U256;

use super::state::token_pricing::TokenPriceGenerator;

mod gas;
mod gas_inspector;

/// validation relating to simulations.
#[derive(Clone)]
pub struct SimValidation<DB> {
    gas_calculator: OrderGasCalculations<DB>,
    /// the amount of excess gas available after the order specific swap.
    /// This allows us to set a limit for how close a order can be for computation
    excess_gas_in_wei: U256,
}

impl<DB> SimValidation<DB>
where
    DB: Unpin + Clone + 'static + revm::DatabaseRef + reth_provider::BlockNumReader + Send + Sync,
    <DB as revm::DatabaseRef>::Error: Send + Sync
{
    pub fn new(db: Arc<DB>, angstrom_address: Option<Address>,
    excess_gas_in_wei: U256,
        ) -> Self {
        let gas_calculator = OrderGasCalculations::new(db.clone(), angstrom_address)
            .expect("failed to deploy baseline angstrom for gas calculations");
        Self { gas_calculator ,excess_gas_in_wei}
    }

    pub fn calculate_tob_gas(
        &self,
        order: &OrderWithStorageData<TopOfBlockOrder>,
        conversion: &TokenPriceGenerator
    ) -> eyre::Result<GasUsed> {
        let gas_in_wei = self.gas_calculator.gas_of_tob_order(order)?;
        // grab order tokens;
        let (token0, token1) = if order.assetIn < order.assetOut {
            (order.assetIn, order.assetOut)
        } else {
            (order.assetOut, order.assetIn)
        };

       // grab price conversion
        let conversion = conversion.get_eth_conversion_price(token0, token1).unwrap();
        let gas_in_token_0
    }

    pub fn calculate_user_gas(
        &self,
        order: &OrderWithStorageData<GroupedVanillaOrder>,
        conversion: &TokenPriceGenerator
    ) -> eyre::Result<GasUsed> {
        // TODO: will do this in next pr but should have the conversion to ERC-20 here
        let gas_in_wei = self.gas_calculator.gas_of_book_order(order)?;
    }
}
