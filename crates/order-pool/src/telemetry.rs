use std::collections::VecDeque;

use angstrom_types::{orders::OrderOrigin, sol_bindings::grouped_orders::AllOrders};
use serde::{Deserialize, Serialize};
use telemetry_recorder::{OrderTelemetryExt, TelemetryMessage};
use validation::order::OrderValidatorHandle;

use crate::{OrderIndexer, order_storage::OrderStorage, order_tracker::OrderTracker};

// Didn't want to have to do a massive refactor of the order pool for this.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderPoolSnapshot {
    pub baseline_for_block: u64,
    pub order_storage: OrderStorage,
    pub order_tracker: OrderTracker,
    pub pending_transition_validation_orders: VecDeque<(OrderOrigin, AllOrders)>
}

impl<V> From<(u64, &OrderIndexer<V>)> for OrderPoolSnapshot
where
    V: OrderValidatorHandle<Order = AllOrders>
{
    fn from((block, value): (u64, &OrderIndexer<V>)) -> Self {
        let storage = value.order_storage.deep_clone();
        let tracker = value.order_tracker.clone();
        let pending = value.validator.get_waiting_orders();
        Self {
            baseline_for_block: block,
            order_tracker: tracker,
            order_storage: storage,
            pending_transition_validation_orders: pending
        }
    }
}

impl OrderTelemetryExt for OrderPoolSnapshot {
    fn into_message(self) -> TelemetryMessage {
        TelemetryMessage::OrderPoolSnapshot {
            blocknum:           self.baseline_for_block,
            orderpool_snapshot: serde_json::to_value(self).unwrap()
        }
    }
}
