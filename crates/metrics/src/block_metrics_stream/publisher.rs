use angstrom_rpc_types::MetricsEvent;

use super::{event::to_envelope, registry::publish};

pub fn publish_block_metrics_event(event: impl Into<MetricsEvent>) {
    if let Some(envelope) = to_envelope(event) {
        publish(envelope);
    }
}
