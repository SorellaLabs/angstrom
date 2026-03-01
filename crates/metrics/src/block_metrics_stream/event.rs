use std::time::{SystemTime, UNIX_EPOCH};

use angstrom_rpc_types::{MetricsEvent, metrics::MetricsEventEnvelope};

use super::meta::stream_metadata;

fn unix_ms_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

pub fn to_envelope(event: impl Into<MetricsEvent>) -> Option<MetricsEventEnvelope> {
    let meta = stream_metadata()?;
    Some(MetricsEventEnvelope {
        node_address:        meta.node_address,
        chain_id:            meta.chain_id,
        // Capture producer-side timing before WS transport and downstream queueing.
        observed_at_unix_ms: unix_ms_now(),
        event:               event.into()
    })
}
