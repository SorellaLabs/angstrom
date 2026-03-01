use angstrom_rpc_types::{MetricsEvent, MetricsEventEnvelope};

use super::registry::publish;
use crate::block_metrics_stream::meta::stream_metadata;

pub fn publish_block_metrics_event(event: impl Into<MetricsEvent>) {
    if let Some(meta) = stream_metadata() {
        publish(MetricsEventEnvelope::new(meta.node_address, meta.chain_id, event));
    }
}
