use std::collections::HashMap;

use alloy::{
    eips::BlockNumHash,
    primitives::{Address, B256}
};
use angstrom_types::{
    contract_payloads::angstrom::BundleGasDetails,
    orders::PoolSolution,
    primitive::PoolId,
    sol_bindings::{grouped_orders::OrderWithStorageData, rpc_orders::TopOfBlockOrder},
    uni_structure::BaselinePoolState
};
use futures::{FutureExt, future::BoxFuture};
use matching_engine::{MatchingEngineHandle, book::BookOrder, manager::MatchingEngineError};

#[derive(Clone)]
pub struct MockMatchingEngine {}

impl MatchingEngineHandle for MockMatchingEngine {
    fn solve_pools(
        &self,
        _: Vec<BookOrder>,
        _: Vec<OrderWithStorageData<TopOfBlockOrder>>,
        _: HashMap<PoolId, (Address, Address, BaselinePoolState, u16)>,
        parent_hash: B256
    ) -> BoxFuture<'_, Result<(Vec<PoolSolution>, BundleGasDetails), MatchingEngineError>> {
        // Echoes the hash back rather than defaulting it, so a test cannot pass on a
        // parent the caller never asked for. The height is not something a mock can
        // resolve, so it stays zero.
        async move { Ok((vec![], BundleGasDetails::new(0, BlockNumHash::new(0, parent_hash)))) }
            .boxed()
    }
}
