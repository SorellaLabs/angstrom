use std::{collections::HashMap, sync::Arc};

use alloy::primitives::{Address, U256};
use angstrom_types::sol_bindings::{
    grouped_orders::{GroupedVanillaOrder, OrderWithStorageData},
    rpc_orders::TopOfBlockOrder
};
use gas::OrderGasCalculations;
use gas_inspector::GasUsed;

use super::OrderValidationRequest;
use crate::common::db::BlockStateProviderFactory;

mod gas;
mod gas_inspector;

/// validation relating to simulations.
#[derive(Clone)]
pub struct SimValidation<DB> {
    db:             Arc<DB>,
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

        Self { db, gas_calculator }
    }

    pub fn calculate_tob_gas(
        &self,
        order: &OrderWithStorageData<TopOfBlockOrder>
    ) -> eyre::Result<GasUsed> {
        // TODO: will do this in next pr but should have the conversion to ERC-20 here
        self.gas_calculator.gas_of_tob_order(order)
    }

    pub fn calculate_user_gas(
        &self,
        order: &OrderWithStorageData<GroupedVanillaOrder>
    ) -> eyre::Result<GasUsed> {
        // TODO: will do this in next pr but should have the conversion to ERC-20 here
        self.gas_calculator.gas_of_book_order(order)
    }

    pub fn validate_hook(
        &self,
        order: OrderValidationRequest
    ) -> (OrderValidationRequest, HashMap<Address, HashMap<U256, U256>>) {
        todo!()
    }

    pub fn validate_post_hook(
        &self,
        order: OrderValidationRequest,
        overrides: HashMap<Address, HashMap<U256, U256>>
    ) -> (OrderValidationRequest, HashMap<Address, HashMap<U256, U256>>) {
        todo!()
    }
}
