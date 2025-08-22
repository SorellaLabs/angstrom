use std::collections::{HashMap, HashSet};

use alloy::primitives::B256;
use angstrom_types::sol_bindings::grouped_orders::{AllOrders, OrderWithStorageData};
use tokio::sync::oneshot::Sender;
use validation::order::OrderValidationResults;

use crate::PoolManagerUpdate;

pub struct OrderSubscriptionTracker {
    /// List of subscribers for order validation result
    order_validation_subs:      HashMap<B256, Vec<Sender<OrderValidationResults>>>,
    /// List of subscribers for order state change notifications
    orders_subscriber_tx:       tokio::sync::broadcast::Sender<PoolManagerUpdate>,
    /// tracks updates for order hashes to ensure we only send out the
    /// appropriate updates once
    order_notification_tracker: HashSet<B256>
}

impl OrderSubscriptionTracker {
    pub fn new(orders_subscriber_tx: tokio::sync::broadcast::Sender<PoolManagerUpdate>) -> Self {
        Self {
            order_validation_subs: Default::default(),
            orders_subscriber_tx,
            order_notification_tracker: HashSet::new()
        }
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
        // if we insert the order and its not a new one, and this isn't the
        // last notification for a order, return
        let id = update.order_id();
        if self.order_notification_tracker.contains(&id) && !update.last_notification_for_order() {
            return;
        }

        // will never be seen again
        if update.last_notification_for_order() {
            self.order_notification_tracker.remove(&id);
        }

        let _ = self.orders_subscriber_tx.send(update);
    }

    pub fn notify_expired_orders(&mut self, orders: &[OrderWithStorageData<AllOrders>]) {
        for order in orders {
            self.order_notification_tracker.remove(&order.order_id.hash);
            let _ = self
                .orders_subscriber_tx
                .send(PoolManagerUpdate::ExpiredOrder(order.clone()));
        }
    }

    pub fn try_notify_validation_subscribers(
        &mut self,
        hash: &B256,
        result: OrderValidationResults
    ) {
        let Some(subscribers) = self.order_validation_subs.remove(hash) else { return };

        for subscriber in subscribers {
            let _ = subscriber.send(result.clone());
        }
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
