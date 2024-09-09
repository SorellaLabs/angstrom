use prometheus::IntGauge;

use crate::METRICS_ENABLED;

#[derive(Clone)]
struct FinalizationOrderPoolMetrics {
    // number of blocks tracked
    blocks_tracked: IntGauge,
    // number of finalization orders
    total_orders:   IntGauge
}

impl Default for FinalizationOrderPoolMetrics {
    fn default() -> Self {
        let total_orders = prometheus::register_int_gauge!(
            "finalization_order_pool_total_orders",
            "number of finalization orders",
        )
        .unwrap();

        let blocks_tracked = prometheus::register_int_gauge!(
            "finalization_order_pool_blocks_tracked",
            "number of blocks tracked",
        )
        .unwrap();

        Self { total_orders, blocks_tracked }
    }
}

impl FinalizationOrderPoolMetrics {
    pub fn incr_total_orders(&self) {
        self.total_orders.add(1);
    }

    pub fn decr_total_orders(&self) {
        self.total_orders.sub(1);
    }

    pub fn incr_blocks_tracked(&self) {
        self.blocks_tracked.add(1);
    }

    pub fn decr_blocks_tracked(&self) {
        self.blocks_tracked.sub(1);
    }
}

#[derive(Clone)]
pub struct FinalizationOrderPoolMetricsWrapper(Option<FinalizationOrderPoolMetrics>);

impl Default for FinalizationOrderPoolMetricsWrapper {
    fn default() -> Self {
        Self::new()
    }
}

impl FinalizationOrderPoolMetricsWrapper {
    pub fn new() -> Self {
        Self(
            METRICS_ENABLED
                .get()
                .copied()
                .unwrap_or_default()
                .then(FinalizationOrderPoolMetrics::default)
        )
    }

    pub fn incr_total_orders(&self) {
        if let Some(this) = self.0.as_ref() {
            this.incr_total_orders()
        }
    }

    pub fn decr_total_orders(&self) {
        if let Some(this) = self.0.as_ref() {
            this.decr_total_orders()
        }
    }

    pub fn incr_blocks_tracked(&self) {
        if let Some(this) = self.0.as_ref() {
            this.incr_blocks_tracked()
        }
    }

    pub fn decr_blocks_tracked(&self) {
        if let Some(this) = self.0.as_ref() {
            this.decr_blocks_tracked()
        }
    }
}
