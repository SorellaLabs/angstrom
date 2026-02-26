mod config;
mod event;
mod health;
mod stats;
mod util;
mod writer;

use std::sync::{Arc, OnceLock};

use angstrom_types::primitive::CHAIN_ID;
pub use config::BlockMetricsDbConfig;
pub(crate) use event::BlockMetricEvent;
use health::BlockMetricsDbWriterMetrics;
use stats::QueueStats;
use tokio::sync::mpsc::{self, error::TrySendError};
use writer::BlockMetricsDbWriter;

use crate::METRICS_ENABLED;

struct BlockMetricsDbQueue {
    sender:         mpsc::Sender<BlockMetricEvent>,
    stats:          Arc<QueueStats>,
    writer_metrics: BlockMetricsDbWriterMetrics
}

static BLOCK_METRICS_DB_QUEUE: OnceLock<BlockMetricsDbQueue> = OnceLock::new();

pub fn initialize_block_metrics_db(
    config: BlockMetricsDbConfig,
    node_address: String
) -> eyre::Result<()> {
    if !METRICS_ENABLED.get().copied().unwrap_or_default() {
        return Ok(());
    }

    config.validate()?;

    if BLOCK_METRICS_DB_QUEUE.get().is_some() {
        return Ok(());
    }

    let writer_metrics = BlockMetricsDbWriterMetrics::new();
    let stats = Arc::new(QueueStats::default());
    let (sender, receiver) = mpsc::channel(config.queue_capacity);

    let queue = BlockMetricsDbQueue {
        sender,
        stats: stats.clone(),
        writer_metrics: writer_metrics.clone()
    };

    BLOCK_METRICS_DB_QUEUE
        .set(queue)
        .map_err(|_| eyre::eyre!("block metrics db queue already initialized"))?;

    let chain_id = CHAIN_ID.get().copied().unwrap_or(1) as i64;
    let writer = BlockMetricsDbWriter::new(
        config,
        node_address,
        chain_id,
        receiver,
        stats.clone(),
        writer_metrics.clone()
    );

    writer_metrics.set_depths(stats.depth_snapshot());
    tokio::spawn(async move {
        writer.run().await;
    });

    Ok(())
}

pub(crate) fn enqueue_block_metric_event(event: BlockMetricEvent) {
    if !METRICS_ENABLED.get().copied().unwrap_or_default() {
        return;
    }

    let Some(queue) = BLOCK_METRICS_DB_QUEUE.get() else {
        return;
    };

    enqueue_event_non_blocking(&queue.sender, &queue.stats, &queue.writer_metrics, event);
}

fn enqueue_event_non_blocking(
    sender: &mpsc::Sender<BlockMetricEvent>,
    stats: &QueueStats,
    writer_metrics: &BlockMetricsDbWriterMetrics,
    event: BlockMetricEvent
) {
    match sender.try_send(event) {
        Ok(()) => {
            let snapshot = stats.on_enqueue_success();
            writer_metrics.set_depths(snapshot);
        }
        Err(TrySendError::Full(_)) | Err(TrySendError::Closed(_)) => {
            stats.increment_enqueue_dropped();
            writer_metrics.increment_enqueue_dropped();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    #[tokio::test]
    async fn enqueue_is_non_blocking_and_tracks_drops_when_full() {
        let (tx, _rx) = mpsc::channel(1);
        let stats = QueueStats::default();
        let metrics = BlockMetricsDbWriterMetrics::disabled();
        let event = BlockMetricEvent::IsLeader { block: 1, is_leader: true };

        let started = Instant::now();
        enqueue_event_non_blocking(&tx, &stats, &metrics, event.clone());
        enqueue_event_non_blocking(&tx, &stats, &metrics, event);

        assert!(
            started.elapsed() < Duration::from_millis(50),
            "enqueue path should not block when queue is full"
        );
        assert_eq!(stats.channel_depth(), 1);
        assert_eq!(stats.buffered_events(), 0);
        assert_eq!(stats.pending_events(), 1);
        assert_eq!(stats.enqueue_dropped(), 1);
    }
}
