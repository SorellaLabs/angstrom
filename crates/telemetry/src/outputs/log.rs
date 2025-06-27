use std::pin::Pin;

use tracing::Level;

use super::TelemetryOutput;
use crate::blocklog::BlockLog;

pub struct LogOutput;

impl TelemetryOutput for LogOutput {
    fn output<'a>(
        &'a self,
        blocklog: BlockLog
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'static>> {
        Box::pin(async move {
            // Dump the solution
            let b64_output = blocklog.to_deflate_base64_str();
            tracing::event!(target: "telemetry_output", Level::INFO, data = b64_output, "Snapshot dump");
        })
    }
}
