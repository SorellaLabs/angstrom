use std::{future::Future, ops::RangeInclusive};

use alloy::rpc::types::eth::Filter;
use alloy_primitives::Log;

use crate::uniswap::pool_manager::PoolManagerError;
pub mod canonical_state_adapter;
pub mod mock_block_stream;
pub mod provider_adapter;

pub trait PoolManagerProvider: Send + Sync {
    fn subscribe_blocks(&self) -> futures::stream::BoxStream<Option<PoolMangerBlocks>>;
    fn get_logs(
        &self,
        filter: &Filter
    ) -> impl Future<Output = Result<Vec<Log>, PoolManagerError>> + Send;
}

#[derive(Debug, Clone)]
pub enum PoolMangerBlocks {
    NewBlock(u64),
    Reorg(u64, RangeInclusive<u64>)
}
