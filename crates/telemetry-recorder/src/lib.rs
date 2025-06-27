use std::{
    collections::{HashMap, VecDeque},
    sync::OnceLock
};

use angstrom_types::{
    consensus::{ConsensusRoundName, StromConsensusEvent},
    contract_bindings::angstrom::Angstrom::PoolKey,
    orders::{CancelOrderRequest, OrderOrigin},
    pair_with_price::PairsWithPrice,
    primitive::PoolId,
    sol_bindings::grouped_orders::AllOrders,
    uni_structure::BaselinePoolState
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedSender;

pub static TELEMETRY_SENDER: OnceLock<UnboundedSender<TelemetryMessage>> = OnceLock::new();

#[macro_export]
macro_rules! telemetry_event {
    // Ext paths will only have 1 argument while non will have 2 or more
    ($update:expr) => {{
        if let Some(handle) = $crate::TELEMETRY_SENDER.get() {
            let message = $crate::OrderTelemetryExt::into_message($update);
            let _ = handle.send(message);
        } else {
            ::tracing::warn!("No Telemetry handle set.");
        }

    }};
    ($($items:expr),*) => {{
        if let Some(handle) = $crate::TELEMETRY_SENDER.get() {
            let message = $crate::TelemetryMessage::from(($($items),*));
            let _ = handle.send(message);
        } else {
            ::tracing::warn!("No Telemetry handle set.");
        }
    }};

}

pub trait OrderTelemetryExt {
    fn into_message(self) -> TelemetryMessage;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TelemetryMessage {
    // don't want to do a big refactor for the given types.
    OrderPoolSnapshot {
        blocknum:           u64,
        orderpool_snapshot: serde_json::Value
    },
    ValidationSnapshot {
        blocknum:            u64,
        validation_snapshot: serde_json::Value
    },
    EthSnapshot {
        blocknum:     u64,
        eth_snapshot: serde_json::Value
    },
    /// Message indicating that a new block has begun.  Sent by the pool manager
    /// with the updated pool snapshot for that block
    NewBlock {
        blocknum:       u64,
        pool_keys:      Vec<PoolKey>,
        pool_snapshots: HashMap<PoolId, BaselinePoolState>
    },
    /// Message indicating an incoming order to be validated
    NewOrder {
        blocknum:  u64,
        timestamp: chrono::DateTime<Utc>,
        origin:    OrderOrigin,
        order:     AllOrders
    },
    /// Request to cancel an order
    CancelOrder {
        blocknum:  u64,
        timestamp: chrono::DateTime<Utc>,
        cancel:    CancelOrderRequest
    },
    /// Message indicating an incoming Consensus message
    Consensus {
        blocknum:  u64,
        timestamp: chrono::DateTime<Utc>,
        event:     StromConsensusEvent
    },
    ConsensusStateChange {
        blocknum:  u64,
        timestamp: chrono::DateTime<Utc>,
        state:     ConsensusRoundName
    },
    /// Message assigning a snapshot of our gas prices to this block
    GasPriceSnapshot {
        blocknum: u64,
        snapshot: (HashMap<PoolId, VecDeque<PairsWithPrice>>, u128)
    },
    /// Message indicating an error has happened, marking a block for output
    Error {
        blocknum:  u64,
        timestamp: chrono::DateTime<Utc>,
        message:   String,
        backtrace: String
    }
}

impl TelemetryMessage {
    pub fn try_get_timestamp(&self) -> chrono::DateTime<Utc> {
        match self {
            TelemetryMessage::NewOrder { timestamp, .. } => *timestamp,
            TelemetryMessage::CancelOrder { timestamp, .. } => *timestamp,
            TelemetryMessage::Consensus { timestamp, .. } => *timestamp,
            TelemetryMessage::ConsensusStateChange { timestamp, .. } => *timestamp,
            _ => panic!("this event isn't timestamped")
        }
    }
}

impl From<(u64, Vec<PoolKey>, HashMap<PoolId, BaselinePoolState>)> for TelemetryMessage {
    fn from(value: (u64, Vec<PoolKey>, HashMap<PoolId, BaselinePoolState>)) -> Self {
        Self::NewBlock { blocknum: value.0, pool_keys: value.1, pool_snapshots: value.2 }
    }
}

impl From<(u64, OrderOrigin, AllOrders)> for TelemetryMessage {
    fn from(value: (u64, OrderOrigin, AllOrders)) -> Self {
        Self::NewOrder {
            blocknum:  value.0,
            origin:    value.1,
            order:     value.2,
            timestamp: Utc::now()
        }
    }
}

impl From<(u64, CancelOrderRequest)> for TelemetryMessage {
    fn from(value: (u64, CancelOrderRequest)) -> Self {
        Self::CancelOrder { blocknum: value.0, timestamp: Utc::now(), cancel: value.1 }
    }
}

impl From<(u64, StromConsensusEvent)> for TelemetryMessage {
    fn from(value: (u64, StromConsensusEvent)) -> Self {
        Self::Consensus { blocknum: value.0, timestamp: Utc::now(), event: value.1 }
    }
}

impl From<(u64, ConsensusRoundName)> for TelemetryMessage {
    fn from(value: (u64, ConsensusRoundName)) -> Self {
        Self::ConsensusStateChange { blocknum: value.0, timestamp: Utc::now(), state: value.1 }
    }
}

impl From<(u64, (HashMap<PoolId, VecDeque<PairsWithPrice>>, u128))> for TelemetryMessage {
    fn from(value: (u64, (HashMap<PoolId, VecDeque<PairsWithPrice>>, u128))) -> Self {
        Self::GasPriceSnapshot { blocknum: value.0, snapshot: value.1 }
    }
}

impl From<(u64, String)> for TelemetryMessage {
    fn from(value: (u64, String)) -> Self {
        let bt = std::backtrace::Backtrace::force_capture();

        Self::Error {
            blocknum:  value.0,
            timestamp: Utc::now(),
            message:   value.1,
            backtrace: bt.to_string()
        }
    }
}
