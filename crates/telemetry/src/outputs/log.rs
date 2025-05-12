use base64::Engine;
use tracing::Level;

use super::TelemetryOutput;
use crate::blocklog::BlockLog;

pub struct LogOutput {}

impl TelemetryOutput for LogOutput {
    fn output(&self, blocklog: &BlockLog) {
        // Dump the solution
        let json = serde_json::to_string(&blocklog).unwrap();
        let b64_output = base64::prelude::BASE64_STANDARD.encode(json.as_bytes());
        tracing::event!(target: "telemetry_output", Level::INFO, data = b64_output, "Snapshot dump");
    }
}
