use std::sync::OnceLock;

use angstrom_rpc_types::metrics::BlockMetricsEventEnvelope;
use tokio::sync::broadcast;

use super::health::increment_publish_no_subscribers;

const STREAM_CAPACITY: usize = 20_000;

static STREAM_SENDER: OnceLock<broadcast::Sender<BlockMetricsEventEnvelope>> = OnceLock::new();

fn sender() -> &'static broadcast::Sender<BlockMetricsEventEnvelope> {
    STREAM_SENDER.get_or_init(|| {
        let (sender, _) = broadcast::channel(STREAM_CAPACITY);
        sender
    })
}

pub fn publish(envelope: BlockMetricsEventEnvelope) {
    if sender().send(envelope).is_err() {
        increment_publish_no_subscribers();
    }
}

pub fn subscribe() -> broadcast::Receiver<BlockMetricsEventEnvelope> {
    sender().subscribe()
}
