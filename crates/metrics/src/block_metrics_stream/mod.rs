mod event;
mod health;
mod meta;
mod publisher;
mod registry;

use angstrom_rpc_types::metrics::BlockMetricsEventEnvelope;
pub use health::{
    increment_lagged_events, increment_receiver_closed, increment_serialize_failures
};
pub use meta::initialize_stream_metadata;
pub use publisher::publish_block_metrics_event;
use tokio::sync::broadcast;

#[derive(Debug, Clone, Copy, Default)]
pub struct BlockMetricsStreamSource;

impl BlockMetricsStreamSource {
    pub fn subscribe(self) -> broadcast::Receiver<BlockMetricsEventEnvelope> {
        registry::subscribe()
    }
}
