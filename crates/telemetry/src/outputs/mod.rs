use crate::blocklog::BlockLog;

pub mod log;

pub trait TelemetryOutput {
    fn output(&self, blocklog: &BlockLog);
}
