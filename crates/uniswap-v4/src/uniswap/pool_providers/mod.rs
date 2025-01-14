use std::{ops::RangeInclusive, sync::Arc};

use alloy::{providers::Provider, rpc::types::eth::Filter};
use alloy_primitives::Log;

use crate::uniswap::pool_manager::PoolManagerError;
pub mod canonical_state_adapter;
pub mod mock_block_stream;
pub mod provider_adapter;

pub trait PoolManagerProvider: Send + Sync + Clone + Unpin {
    fn subscribe_blocks(self) -> futures::stream::BoxStream<'static, Option<PoolMangerBlocks>>;

    fn get_logs(&self, filter: &Filter) -> Result<Vec<Log>, PoolManagerError>;
    fn provider(&self) -> Arc<impl Provider>;
}

#[derive(Debug, Clone)]
pub enum PoolMangerBlocks {
    NewBlock(u64),
    Reorg(u64, RangeInclusive<u64>)
}
