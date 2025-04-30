use base64::Engine;
use tracing::Level;

use super::TelemetryOutput;

pub struct LogOutput {}

impl TelemetryOutput for LogOutput {
    fn snapshot(&self, snap: &crate::snapshot::Snapshot) {
        // Dump the solution
        let json = serde_json::to_string(&snap).unwrap();
        let b64_output = base64::prelude::BASE64_STANDARD.encode(json.as_bytes());
        tracing::event!(target: "telemetry_output", Level::INFO, data = b64_output, "Snapshot dump");
    }
}
