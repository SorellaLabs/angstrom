use std::sync::OnceLock;

use angstrom_types::primitive::PoolId;
use prometheus::{IntGauge, IntGaugeVec};

use crate::METRICS_ENABLED;

#[derive(Debug, Clone)]
struct SearcherOrderPoolMetrics {
    // number of searcher orders
    total_orders: IntGauge,
    // number of orders per pool
    all_orders:   IntGaugeVec
}

impl Default for SearcherOrderPoolMetrics {
    fn default() -> Self {
        let total_orders = prometheus::register_int_gauge!(
            "ang_searcher_order_pool_total_orders",
            "number of searcher orders",
        )
        .unwrap();

        let all_orders = prometheus::register_int_gauge_vec!(
            "ang_searcher_order_pool_all_orders",
            " number of orders per pool",
            &["pool_id"]
        )
        .unwrap();

        Self { total_orders, all_orders }
    }
}

impl SearcherOrderPoolMetrics {
    fn incr_total_orders(&self, count: usize) {
        self.total_orders.add(count as i64);
    }

    fn decr_total_orders(&self, count: usize) {
        self.total_orders.sub(count as i64);
    }

    pub fn incr_all_orders(&self, pool_id: PoolId, count: usize) {
        self.all_orders
            .get_metric_with_label_values(&[&pool_id.to_string()])
            .unwrap()
            .add(count as i64);

        self.incr_total_orders(count);
    }

    pub fn decr_all_orders(&self, pool_id: PoolId, count: usize) {
        self.all_orders
            .get_metric_with_label_values(&[&pool_id.to_string()])
            .unwrap()
            .sub(count as i64);

        self.decr_total_orders(count);
    }
}

static METRICS_INSTANCE: OnceLock<SearcherOrderPoolMetricsWrapper> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct SearcherOrderPoolMetricsWrapper(Option<SearcherOrderPoolMetrics>);

impl Default for SearcherOrderPoolMetricsWrapper {
    fn default() -> Self {
        Self::new()
    }
}

impl SearcherOrderPoolMetricsWrapper {
    pub fn new() -> Self {
        METRICS_INSTANCE
            .get_or_init(|| {
                Self(
                    METRICS_ENABLED
                        .get()
                        .copied()
                        .unwrap_or_default()
                        .then(SearcherOrderPoolMetrics::default)
                )
            })
            .clone()
    }

    pub fn incr_total_orders(&self, count: usize) {
        if let Some(this) = self.0.as_ref() {
            this.incr_total_orders(count)
        }
    }

    pub fn decr_total_orders(&self, count: usize) {
        if let Some(this) = self.0.as_ref() {
            this.decr_total_orders(count)
        }
    }

    pub fn incr_all_orders(&self, pool_id: PoolId, count: usize) {
        if let Some(this) = self.0.as_ref() {
            this.incr_all_orders(pool_id, count)
        }
    }

    pub fn decr_all_orders(&self, pool_id: PoolId, count: usize) {
        if let Some(this) = self.0.as_ref() {
            this.decr_all_orders(pool_id, count)
        }
    }
}
