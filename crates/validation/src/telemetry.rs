use serde::{Deserialize, Serialize};
use telemetry_recorder::OrderTelemetryExt;

use crate::order::state::account::user::UserAccounts;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationSnapshot {
    pub block_number: u64,
    pub state:        UserAccounts
}

impl From<(u64, UserAccounts)> for ValidationSnapshot {
    fn from(value: (u64, UserAccounts)) -> Self {
        Self { block_number: value.0, state: value.1 }
    }
}

impl OrderTelemetryExt for ValidationSnapshot {
    fn into_message(self) -> telemetry_recorder::TelemetryMessage {
        telemetry_recorder::TelemetryMessage::ValidationSnapshot {
            blocknum:            self.block_number,
            validation_snapshot: serde_json::to_value(self).unwrap()
        }
    }
}
