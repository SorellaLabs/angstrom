use std::sync::OnceLock;

use prometheus::IntCounter;

#[derive(Clone)]
pub(crate) struct StreamHealthMetrics {
    publish_no_subscribers_total: IntCounter,
    lagged_events_total:          IntCounter,
    receiver_closed_total:        IntCounter,
    serialize_failures_total:     IntCounter
}

impl StreamHealthMetrics {
    fn new() -> Self {
        let publish_no_subscribers_total = prometheus::register_int_counter!(
            "ang_metrics_stream_publish_no_subscribers_total",
            "Number of block-metrics stream events dropped because no subscriber was attached"
        )
        .unwrap();
        let lagged_events_total = prometheus::register_int_counter!(
            "ang_metrics_stream_lagged_events_total",
            "Number of block-metrics stream events skipped due to subscriber lag"
        )
        .unwrap();
        let receiver_closed_total = prometheus::register_int_counter!(
            "ang_metrics_stream_receiver_closed_total",
            "Number of block-metrics stream receiver closures observed by the metrics RPC"
        )
        .unwrap();
        let serialize_failures_total = prometheus::register_int_counter!(
            "ang_metrics_stream_serialize_failures_total",
            "Number of block-metrics stream serialization failures before WS send"
        )
        .unwrap();

        Self {
            publish_no_subscribers_total,
            lagged_events_total,
            receiver_closed_total,
            serialize_failures_total
        }
    }
}

static STREAM_HEALTH_METRICS: OnceLock<StreamHealthMetrics> = OnceLock::new();

fn metrics() -> &'static StreamHealthMetrics {
    STREAM_HEALTH_METRICS.get_or_init(StreamHealthMetrics::new)
}

pub(crate) fn increment_publish_no_subscribers() {
    metrics().publish_no_subscribers_total.inc();
}

pub fn increment_lagged_events(dropped: u64) {
    metrics().lagged_events_total.inc_by(dropped);
}

pub fn increment_receiver_closed() {
    metrics().receiver_closed_total.inc();
}

pub fn increment_serialize_failures() {
    metrics().serialize_failures_total.inc();
}
