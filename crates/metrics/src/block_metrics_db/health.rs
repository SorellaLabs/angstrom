use std::sync::OnceLock;

use prometheus::{IntCounter, IntGauge};

use super::stats::QueueDepthSnapshot;
use crate::METRICS_ENABLED;

#[derive(Clone)]
pub(crate) struct BlockMetricsDbWriterMetrics(Option<BlockMetricsDbWriterMetricsInner>);

#[derive(Clone)]
struct BlockMetricsDbWriterMetricsInner {
    queue_depth:           IntGauge,
    channel_depth:         IntGauge,
    buffered_events:       IntGauge,
    enqueue_dropped_total: IntCounter,
    write_failures_total:  IntCounter,
    flush_success_total:   IntCounter,
    last_flush_unixtime:   IntGauge,
    retention_deletes:     IntCounter
}

impl BlockMetricsDbWriterMetrics {
    pub(crate) fn new() -> Self {
        static WRITER_METRICS_INSTANCE: OnceLock<BlockMetricsDbWriterMetrics> = OnceLock::new();

        WRITER_METRICS_INSTANCE
            .get_or_init(|| {
                if !METRICS_ENABLED.get().copied().unwrap_or_default() {
                    return Self(None);
                }

                let queue_depth = prometheus::register_int_gauge!(
                    "ang_block_metrics_db_queue_depth",
                    "current pending event depth for block metrics db writer (channel + buffer)"
                )
                .unwrap();

                let channel_depth = prometheus::register_int_gauge!(
                    "ang_block_metrics_db_channel_depth",
                    "current channel depth for block metrics db writer"
                )
                .unwrap();

                let buffered_events = prometheus::register_int_gauge!(
                    "ang_block_metrics_db_buffered_events",
                    "current in-memory buffered events awaiting db flush"
                )
                .unwrap();

                let enqueue_dropped_total = prometheus::register_int_counter!(
                    "ang_block_metrics_db_enqueue_dropped_total",
                    "number of block metric events dropped due to full queue or closed channel"
                )
                .unwrap();

                let write_failures_total = prometheus::register_int_counter!(
                    "ang_block_metrics_db_write_failures_total",
                    "number of db flush or retention failures for block metrics writer"
                )
                .unwrap();

                let flush_success_total = prometheus::register_int_counter!(
                    "ang_block_metrics_db_flush_success_total",
                    "number of successful db flushes for block metrics writer"
                )
                .unwrap();

                let last_flush_unixtime = prometheus::register_int_gauge!(
                    "ang_block_metrics_db_last_flush_unixtime",
                    "unix timestamp of the last successful db flush"
                )
                .unwrap();

                let retention_deletes = prometheus::register_int_counter!(
                    "ang_block_metrics_db_retention_deletes_total",
                    "number of rows deleted by retention for block metrics db tables"
                )
                .unwrap();

                Self(Some(BlockMetricsDbWriterMetricsInner {
                    queue_depth,
                    channel_depth,
                    buffered_events,
                    enqueue_dropped_total,
                    write_failures_total,
                    flush_success_total,
                    last_flush_unixtime,
                    retention_deletes
                }))
            })
            .clone()
    }

    #[cfg(test)]
    pub(crate) fn disabled() -> Self {
        Self(None)
    }

    pub(crate) fn set_depths(&self, snapshot: QueueDepthSnapshot) {
        if let Some(inner) = &self.0 {
            inner.queue_depth.set(snapshot.pending_events.max(0));
            inner.channel_depth.set(snapshot.channel_depth.max(0));
            inner.buffered_events.set(snapshot.buffered_events.max(0));
        }
    }

    pub(crate) fn increment_enqueue_dropped(&self) {
        if let Some(inner) = &self.0 {
            inner.enqueue_dropped_total.inc();
        }
    }

    pub(crate) fn increment_write_failures(&self) {
        if let Some(inner) = &self.0 {
            inner.write_failures_total.inc();
        }
    }

    pub(crate) fn increment_flush_success(&self) {
        if let Some(inner) = &self.0 {
            inner.flush_success_total.inc();
        }
    }

    pub(crate) fn set_last_flush_unixtime(&self, unix_ts: i64) {
        if let Some(inner) = &self.0 {
            inner.last_flush_unixtime.set(unix_ts);
        }
    }

    pub(crate) fn add_retention_deletes(&self, deleted: u64) {
        if let Some(inner) = &self.0 {
            inner.retention_deletes.inc_by(deleted);
        }
    }
}
