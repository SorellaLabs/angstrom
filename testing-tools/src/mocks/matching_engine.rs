use std::collections::HashMap;

use alloy::primitives::Address;
use angstrom_types::{
    contract_payloads::angstrom::BundleGasDetails,
    matching::uniswap::PoolSnapshot,
    orders::PoolSolution,
    primitive::PoolId,
    sol_bindings::{grouped_orders::OrderWithStorageData, rpc_orders::TopOfBlockOrder}
};
use futures::{future::BoxFuture, FutureExt};
use matching_engine::{book::BookOrder, MatchingEngineHandle};

#[derive(Clone)]
pub struct MockMatchingEngine {}

impl MatchingEngineHandle for MockMatchingEngine {
    fn solve_pools(
        &self,
        _: Vec<BookOrder>,
        _: Vec<OrderWithStorageData<TopOfBlockOrder>>,
        _: HashMap<PoolId, (Address, Address, PoolSnapshot, u16)>
    ) -> BoxFuture<eyre::Result<(Vec<PoolSolution>, BundleGasDetails)>> {
        async move { Ok((vec![], BundleGasDetails::default())) }.boxed()
    }
}
