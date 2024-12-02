use angstrom_types::{
    primitive::{AngstromSigner, PoolId},
    sol_bindings::{grouped_orders::GroupedVanillaOrder, rpc_orders::TopOfBlockOrder}
};
use uniswap_v4::uniswap::pool_manager::SyncedUniswapPools;

use crate::type_generator::orders::ToBOrderBuilder;

pub struct OrderBuilder {
    keys:      Vec<AngstromSigner>,
    pool_id:   PoolId,
    /// pools to based orders off of
    pool_data: SyncedUniswapPools
}

impl OrderBuilder {
    pub fn new(pool_id: PoolId, pool_data: SyncedUniswapPools) -> Self {
        Self { keys: vec![], pool_id, pool_data }
    }

    pub fn build_tob_order(&self, cur_price: f64, block_number: u64) -> TopOfBlockOrder {
        // ToBOrderBuilder::new().sig

        todo!()
    }

    pub fn build_user_order(
        &self,
        cur_price: f64,
        block_number: u64,
        is_partial: bool
    ) -> GroupedVanillaOrder {
        todo!()
    }
}
