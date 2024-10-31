use std::sync::Arc;

use alloy::primitives::Address;
use angstrom_types::sol_bindings::{
    grouped_orders::{GroupedVanillaOrder, OrderWithStorageData},
    rpc_orders::TopOfBlockOrder,
    RawPoolOrder
};
use gas::OrderGasCalculations;
use revm::primitives::ruint::aliases::U256;

use super::state::token_pricing::TokenPriceGenerator;
use crate::order::state::token_pricing::WETH_ADDRESS;

mod gas;
mod gas_inspector;

pub type GasInToken0 = U256;
/// validation relating to simulations.
#[derive(Clone)]
pub struct SimValidation<DB> {
    gas_calculator: OrderGasCalculations<DB>
}

impl<DB> SimValidation<DB>
where
    DB: Unpin + Clone + 'static + revm::DatabaseRef + reth_provider::BlockNumReader + Send + Sync,
    <DB as revm::DatabaseRef>::Error: Send + Sync
{
    pub fn new(db: Arc<DB>, angstrom_address: Option<Address>) -> Self {
        let gas_calculator = OrderGasCalculations::new(db.clone(), angstrom_address)
            .expect("failed to deploy baseline angstrom for gas calculations");
        Self { gas_calculator }
    }

    pub fn calculate_tob_gas(
        &self,
        order: &OrderWithStorageData<TopOfBlockOrder>,
        conversion: &TokenPriceGenerator
    ) -> eyre::Result<GasInToken0> {
        let gas_in_wei = self.gas_calculator.gas_of_tob_order(order)?;
        // grab order tokens;
        let (token0, token1) = if order.asset_in < order.asset_out {
            (order.asset_in, order.asset_out)
        } else {
            (order.asset_out, order.asset_in)
        };

        // grab price conversion
        let conversion_factor = if token0 < token1 && token0 == WETH_ADDRESS {
            U256::from(1)
        } else {
            conversion.get_eth_conversion_price(token0, token1).unwrap()
        };
        Ok(conversion_factor * U256::from(gas_in_wei))
    }

    pub fn calculate_user_gas(
        &self,
        order: &OrderWithStorageData<GroupedVanillaOrder>,
        conversion: &TokenPriceGenerator
    ) -> eyre::Result<GasInToken0> {
        let gas_in_wei = self.gas_calculator.gas_of_book_order(order)?;
        // grab order tokens;
        let (token0, token1) = if order.token_in() < order.token_out() {
            (order.token_in(), order.token_out())
        } else {
            (order.token_out(), order.token_in())
        };

        // grab price conversion
        let conversion_factor = if token0 < token1 && token0 == WETH_ADDRESS {
            U256::from(1)
        } else {
            conversion.get_eth_conversion_price(token0, token1).unwrap()
        };
        Ok(conversion_factor * U256::from(gas_in_wei))
    }
}
