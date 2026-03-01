use angstrom_rpc_types::metrics::BlockMetricsEvent;

use super::{event::to_envelope, registry::publish};

pub fn publish_block_metrics_event(event: BlockMetricsEvent) {
    if let Some(envelope) = to_envelope(event) {
        publish(envelope);
    }
}
