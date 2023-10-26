pub mod handle;
pub mod manager;
pub mod relay_sender;
pub mod round_robin_sync;

use ethers_core::types::{Block, H256};
use ethers_flashbots::PendingBundleError;
use tokio_stream::wrappers::ReceiverStream;

#[async_trait::async_trait]
pub trait EthSubscribe: Send + Sync + 'static {
    async fn subscribe_new_blocks(&self) -> ReceiverStream<Block<H256>>;
    async fn subscribe_sync(&self, start_block: u64) -> ReceiverStream<Block<H256>>;
    async fn subscribe_leader_submission(&self) -> ReceiverStream<Result<(), PendingBundleError>>;
}

// TODO: prob don't need this but just ez template
#[async_trait::async_trait]
pub trait EthQueries: Send + Sync + 'static {
    async fn fetch_block(&self, block: u64) -> Block<H256>;
}
