use prometheus::{IntGauge, IntGaugeVec};

use crate::METRICS_ENABLED;

#[derive(Clone)]
struct VanillaLimitOrderPoolMetrics {
    // number of vanilla limit orders
    total_orders:         IntGauge,
    // number of pending vanilla limit orders
    total_pending_orders: IntGauge,
    // number of parked vanilla limit orders
    total_parked_orders:  IntGauge,
    // number of pending orders per pool
    pending_orders:       IntGaugeVec,
    // number of parked orders per pool
    parked_orders:        IntGaugeVec
}

impl Default for VanillaLimitOrderPoolMetrics {
    fn default() -> Self {
        let total_orders = prometheus::register_int_gauge!(
            "vanilla_limit_order_pool_total_orders",
            "number of total vanilla limit orders",
        )
        .unwrap();

        let total_pending_orders = prometheus::register_int_gauge!(
            "vanilla_limit_order_pool_total_pending_orders",
            "number of pending vanilla limit orders",
        )
        .unwrap();

        let total_parked_orders = prometheus::register_int_gauge!(
            "vanilla_limit_order_pool_total_parked_orders",
            "number of parked vanilla limit orders",
        )
        .unwrap();

        let pending_orders = prometheus::register_int_gauge_vec!(
            "vanilla_limit_order_pool_pending_orders",
            "number of pending orders per pool",
            &["pool_id"]
        )
        .unwrap();

        let parked_orders = prometheus::register_int_gauge_vec!(
            "vanilla_limit_order_pool_parked_orders",
            "number of parked orders per pool",
            &["pool_id"]
        )
        .unwrap();

        Self {
            total_orders,
            parked_orders,
            pending_orders,
            total_parked_orders,
            total_pending_orders
        }
    }
}

impl VanillaLimitOrderPoolMetrics {
    fn incr_total_orders(&self, count: usize) {
        self.total_orders.add(count as i64);
    }

    fn decr_total_orders(&self, count: usize) {
        self.total_orders.sub(count as i64);
    }

    fn incr_total_parked_orders(&self, count: usize) {
        self.total_parked_orders.add(count as i64);
        self.incr_total_orders(count);
    }

    fn decr_total_parked_orders(&self, count: usize) {
        self.total_parked_orders.sub(count as i64);
        self.decr_total_orders(count);
    }

    fn incr_total_pending_orders(&self, count: usize) {
        self.total_pending_orders.add(count as i64);
        self.incr_total_orders(count);
    }

    fn decr_total_pending_orders(&self, count: usize) {
        self.total_pending_orders.sub(count as i64);
        self.decr_total_orders(count);
    }

    pub fn incr_parked_orders(&self, pool_id: usize, count: usize) {
        self.parked_orders
            .get_metric_with_label_values(&[&pool_id.to_string()])
            .unwrap()
            .add(count as i64);
        self.incr_total_parked_orders(count);
    }

    pub fn decr_parked_orders(&self, pool_id: usize, count: usize) {
        self.parked_orders
            .get_metric_with_label_values(&[&pool_id.to_string()])
            .unwrap()
            .sub(count as i64);
        self.decr_total_parked_orders(count);
    }

    pub fn incr_pending_orders(&self, pool_id: usize, count: usize) {
        self.pending_orders
            .get_metric_with_label_values(&[&pool_id.to_string()])
            .unwrap()
            .add(count as i64);
        self.incr_total_pending_orders(count);
    }

    pub fn decr_pending_orders(&self, pool_id: usize, count: usize) {
        self.pending_orders
            .get_metric_with_label_values(&[&pool_id.to_string()])
            .unwrap()
            .sub(count as i64);
        self.decr_total_pending_orders(count);
    }
}

#[derive(Clone)]
pub struct VanillaLimitOrderPoolMetricsWrapper(Option<VanillaLimitOrderPoolMetrics>);

impl Default for VanillaLimitOrderPoolMetricsWrapper {
    fn default() -> Self {
        Self::new()
    }
}

impl VanillaLimitOrderPoolMetricsWrapper {
    pub fn new() -> Self {
        Self(
            METRICS_ENABLED
                .get()
                .copied()
                .unwrap_or_default()
                .then(VanillaLimitOrderPoolMetrics::default)
        )
    }

    pub fn incr_parked_orders(&self, pool_id: usize, count: usize) {
        if let Some(this) = self.0.as_ref() {
            this.incr_parked_orders(pool_id, count)
        }
    }

    pub fn decr_parked_orders(&self, pool_id: usize, count: usize) {
        if let Some(this) = self.0.as_ref() {
            this.decr_parked_orders(pool_id, count)
        }
    }

    pub fn incr_pending_orders(&self, pool_id: usize, count: usize) {
        if let Some(this) = self.0.as_ref() {
            this.incr_pending_orders(pool_id, count)
        }
    }

    pub fn decr_pending_orders(&self, pool_id: usize, count: usize) {
        if let Some(this) = self.0.as_ref() {
            this.decr_pending_orders(pool_id, count)
        }
    }
}

#[derive(Clone)]
struct ComposableLimitOrderPoolMetrics {
    // number of composable limit orders
    total_orders: IntGauge,
    // number of orders per pool
    all_orders:   IntGaugeVec
}

impl Default for ComposableLimitOrderPoolMetrics {
    fn default() -> Self {
        let total_orders = prometheus::register_int_gauge!(
            "composable_limit_order_pool_total_orders",
            "number of composable limit orders",
        )
        .unwrap();

        let all_orders = prometheus::register_int_gauge_vec!(
            "composable_limit_order_pool_all_orders",
            " number of orders per pool",
            &["pool_id"]
        )
        .unwrap();

        Self { total_orders, all_orders }
    }
}

impl ComposableLimitOrderPoolMetrics {
    fn incr_total_orders(&self, count: usize) {
        self.total_orders.add(count as i64);
    }

    fn decr_total_orders(&self, count: usize) {
        self.total_orders.sub(count as i64);
    }

    pub fn incr_all_orders(&self, pool_id: usize, count: usize) {
        self.all_orders
            .get_metric_with_label_values(&[&pool_id.to_string()])
            .unwrap()
            .add(count as i64);

        self.incr_total_orders(count);
    }

    pub fn decr_all_orders(&self, pool_id: usize, count: usize) {
        self.all_orders
            .get_metric_with_label_values(&[&pool_id.to_string()])
            .unwrap()
            .sub(count as i64);

        self.decr_total_orders(count);
    }
}

#[derive(Clone)]
pub struct ComposableLimitOrderPoolMetricsWrapper(Option<ComposableLimitOrderPoolMetrics>);

impl Default for ComposableLimitOrderPoolMetricsWrapper {
    fn default() -> Self {
        Self::new()
    }
}

impl ComposableLimitOrderPoolMetricsWrapper {
    pub fn new() -> Self {
        Self(
            METRICS_ENABLED
                .get()
                .copied()
                .unwrap_or_default()
                .then(ComposableLimitOrderPoolMetrics::default)
        )
    }

    pub fn incr_all_orders(&self, pool_id: usize, count: usize) {
        if let Some(this) = self.0.as_ref() {
            this.incr_all_orders(pool_id, count)
        }
    }

    pub fn decr_all_orders(&self, pool_id: usize, count: usize) {
        if let Some(this) = self.0.as_ref() {
            this.decr_all_orders(pool_id, count)
        }
    }
}
