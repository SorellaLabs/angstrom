use crate::snapshot::Snapshot;

pub mod log;

pub trait TelemetryOutput {
    fn snapshot(&self, snap: &Snapshot);
}
