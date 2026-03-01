use std::time::{Duration, SystemTime, UNIX_EPOCH};

use angstrom_rpc_types::metrics::{BlockMetricsEvent, BlockMetricsEventEnvelope};

use super::meta::stream_metadata;

fn unix_ms_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_millis() as u64
}

pub fn to_envelope(event: BlockMetricsEvent) -> Option<BlockMetricsEventEnvelope> {
    let meta = stream_metadata()?;
    Some(BlockMetricsEventEnvelope {
        node_address: meta.node_address.clone(),
        chain_id: meta.chain_id,
        // Capture producer-side timing before WS transport and downstream queueing.
        observed_at_unix_ms: unix_ms_now(),
        event
    })
}
