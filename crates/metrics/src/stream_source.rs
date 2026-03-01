use angstrom_rpc_types::MetricsEventEnvelope;
use tokio_stream::wrappers::BroadcastStream;

pub trait MetricsStreamSource: Clone + Send + Sync + 'static {
    fn subscribe_block_events(&self) -> BroadcastStream<MetricsEventEnvelope>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct BlockMetricsStreamSource;

impl MetricsStreamSource for BlockMetricsStreamSource {
    fn subscribe_block_events(&self) -> BroadcastStream<MetricsEventEnvelope> {
        BroadcastStream::new(crate::block_metrics_stream::registry::subscribe())
    }
}
