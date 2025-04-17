use std::collections::HashMap;

use alloy::primitives::Address;
use angstrom_types::{
    contract_payloads::angstrom::BundleGasDetails,
    matching::uniswap::PoolSnapshot,
    orders::PoolSolution,
    primitive::PoolId,
    sol_bindings::{grouped_orders::OrderWithStorageData, rpc_orders::TopOfBlockOrder},
    uni_structure::BaselinePoolState
};
use futures::{FutureExt, future::BoxFuture};
use matching_engine::{MatchingEngineHandle, book::BookOrder};

#[derive(Clone)]
pub struct MockMatchingEngine {}

impl MatchingEngineHandle for MockMatchingEngine {
    fn solve_pools(
        &self,
        _: Vec<BookOrder>,
        _: Vec<OrderWithStorageData<TopOfBlockOrder>>,
        _: HashMap<PoolId, (Address, Address, BaselinePoolState, u16)>
    ) -> BoxFuture<eyre::Result<(Vec<PoolSolution>, BundleGasDetails)>> {
        async move { Ok((vec![], BundleGasDetails::default())) }.boxed()
    }
}
