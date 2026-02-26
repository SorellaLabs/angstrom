use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct QueueDepthSnapshot {
    pub(crate) channel_depth:   i64,
    pub(crate) buffered_events: i64,
    pub(crate) pending_events:  i64
}

impl QueueDepthSnapshot {
    fn from_parts(channel_depth: i64, buffered_events: i64) -> Self {
        Self {
            channel_depth,
            buffered_events,
            pending_events: channel_depth.saturating_add(buffered_events)
        }
    }
}

#[derive(Default)]
pub(crate) struct QueueStats {
    channel_depth:       AtomicI64,
    buffered_events:     AtomicI64,
    enqueue_dropped:     AtomicU64,
    write_failures:      AtomicU64,
    flush_success:       AtomicU64,
    retention_deletes:   AtomicU64,
    last_flush_unixtime: AtomicI64
}

impl QueueStats {
    pub(crate) fn on_enqueue_success(&self) -> QueueDepthSnapshot {
        let channel_depth = self.channel_depth.fetch_add(1, Ordering::Relaxed) + 1;
        let buffered_events = self.buffered_events.load(Ordering::Relaxed);
        QueueDepthSnapshot::from_parts(channel_depth, buffered_events)
    }

    pub(crate) fn on_dequeue_to_buffer(&self) -> QueueDepthSnapshot {
        let channel_depth = saturating_sub_by(&self.channel_depth, 1);
        let buffered_events = self.buffered_events.fetch_add(1, Ordering::Relaxed) + 1;
        QueueDepthSnapshot::from_parts(channel_depth, buffered_events)
    }

    pub(crate) fn on_flush_success(&self, flushed_events: usize) -> QueueDepthSnapshot {
        let flushed_events = i64::try_from(flushed_events).unwrap_or(i64::MAX);
        let buffered_events = saturating_sub_by(&self.buffered_events, flushed_events);
        let channel_depth = self.channel_depth.load(Ordering::Relaxed);
        QueueDepthSnapshot::from_parts(channel_depth, buffered_events)
    }

    pub(crate) fn depth_snapshot(&self) -> QueueDepthSnapshot {
        let channel_depth = self.channel_depth.load(Ordering::Relaxed);
        let buffered_events = self.buffered_events.load(Ordering::Relaxed);
        QueueDepthSnapshot::from_parts(channel_depth, buffered_events)
    }

    pub(crate) fn increment_enqueue_dropped(&self) {
        self.enqueue_dropped.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn increment_write_failures(&self) {
        self.write_failures.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn increment_flush_success(&self) {
        self.flush_success.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn add_retention_deletes(&self, deleted: u64) {
        self.retention_deletes.fetch_add(deleted, Ordering::Relaxed);
    }

    pub(crate) fn set_last_flush_unixtime(&self, ts: i64) {
        self.last_flush_unixtime.store(ts, Ordering::Relaxed);
    }

    #[cfg(test)]
    pub(crate) fn channel_depth(&self) -> i64 {
        self.channel_depth.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    pub(crate) fn buffered_events(&self) -> i64 {
        self.buffered_events.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    pub(crate) fn pending_events(&self) -> i64 {
        self.depth_snapshot().pending_events
    }

    #[cfg(test)]
    pub(crate) fn enqueue_dropped(&self) -> u64 {
        self.enqueue_dropped.load(Ordering::Relaxed)
    }
}

fn saturating_sub_by(value: &AtomicI64, decrement: i64) -> i64 {
    if decrement <= 0 {
        return value.load(Ordering::Relaxed);
    }

    let mut current = value.load(Ordering::Relaxed);
    loop {
        let next = current.saturating_sub(decrement).max(0);
        match value.compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return next,
            Err(actual) => current = actual
        }
    }
}

#[cfg(test)]
mod tests {
    use super::QueueStats;

    #[test]
    fn queue_depth_transitions_track_channel_buffered_and_pending() {
        let stats = QueueStats::default();

        let snapshot = stats.on_enqueue_success();
        assert_eq!(snapshot.channel_depth, 1);
        assert_eq!(snapshot.buffered_events, 0);
        assert_eq!(snapshot.pending_events, 1);

        let snapshot = stats.on_enqueue_success();
        assert_eq!(snapshot.channel_depth, 2);
        assert_eq!(snapshot.buffered_events, 0);
        assert_eq!(snapshot.pending_events, 2);

        let snapshot = stats.on_dequeue_to_buffer();
        assert_eq!(snapshot.channel_depth, 1);
        assert_eq!(snapshot.buffered_events, 1);
        assert_eq!(snapshot.pending_events, 2);

        let snapshot = stats.on_flush_success(1);
        assert_eq!(snapshot.channel_depth, 1);
        assert_eq!(snapshot.buffered_events, 0);
        assert_eq!(snapshot.pending_events, 1);
    }

    #[test]
    fn flush_depth_saturates_at_zero() {
        let stats = QueueStats::default();

        stats.on_enqueue_success();
        stats.on_dequeue_to_buffer();

        let snapshot = stats.on_flush_success(1000);
        assert_eq!(snapshot.channel_depth, 0);
        assert_eq!(snapshot.buffered_events, 0);
        assert_eq!(snapshot.pending_events, 0);
    }
}
