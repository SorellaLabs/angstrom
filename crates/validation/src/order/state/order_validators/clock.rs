use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering}
    },
    time::{Duration, SystemTime, UNIX_EPOCH}
};

/// The execution time admission and pruning judge deadlines against.
///
/// On a live node this is the wall clock, because there the wall clock *is* the
/// execution time. Replay drives it from the recorded timestamps instead, so an
/// order that was valid at the block it was recorded on is still admitted no
/// matter when the replay runs.
#[derive(Clone, Debug, Default)]
pub enum ValidationClock {
    #[default]
    System,
    Replay(Arc<AtomicU64>)
}

impl ValidationClock {
    /// A clock pinned to `start`, advanced with [`ValidationClock::set`].
    pub fn replay(start: Duration) -> Self {
        Self::Replay(Arc::new(AtomicU64::new(start.as_secs())))
    }

    pub fn now(&self) -> Duration {
        match self {
            Self::System => SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock is before the unix epoch"),
            Self::Replay(secs) => Duration::from_secs(secs.load(Ordering::Relaxed))
        }
    }

    /// Advances a replay clock. A no-op on [`ValidationClock::System`], so
    /// callers do not have to know which kind they hold.
    pub fn set(&self, unix_secs: u64) {
        if let Self::Replay(current) = self {
            current.store(unix_secs, Ordering::Relaxed);
        }
    }
}
