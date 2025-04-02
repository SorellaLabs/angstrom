use std::collections::HashMap;

use alloy::primitives::B256;
use tokio::sync::oneshot::Sender;
use validation::order::OrderValidationResults;

use crate::PoolManagerUpdate;

pub struct OrderSubscriptionTracker {
    /// List of subscribers for order validation result
    order_validation_subs: HashMap<B256, Vec<Sender<OrderValidationResults>>>,
    /// List of subscribers for order state change notifications
    orders_subscriber_tx:  tokio::sync::broadcast::Sender<PoolManagerUpdate>
}

impl OrderSubscriptionTracker {
    pub fn new(orders_subscriber_tx: tokio::sync::broadcast::Sender<PoolManagerUpdate>) -> Self {
        Self { order_validation_subs: Default::default(), orders_subscriber_tx }
    }

    pub fn subscribe_to_order(
        &mut self,
        order_hash: B256,
        tx: tokio::sync::oneshot::Sender<OrderValidationResults>
    ) {
        self.order_validation_subs
            .entry(order_hash)
            .or_default()
            .push(tx);
    }

    pub fn notify_order_subscribers(&mut self, update: PoolManagerUpdate) {
        let _ = self.orders_subscriber_tx.send(update);
    }

    pub fn notify_validation_subscribers(&mut self, hash: &B256, result: OrderValidationResults) {
        let Some(subscribers) = self.order_validation_subs.remove(hash) else { return };

        for subscriber in subscribers {
            let _ = subscriber.send(result.clone()).inspect_err(|e| {
                tracing::error!("Failed to send order validation result to subscriber: {:?}", e);
            });
        }
    }
}
