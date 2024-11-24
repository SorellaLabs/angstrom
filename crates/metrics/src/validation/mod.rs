use std::{future::Future, pin::Pin, time::Instant};

use prometheus::{Histogram, HistogramVec, IntGauge};

use crate::METRICS_ENABLED;

pub mod simulations;
pub mod state;
pub use simulations::*;
pub use state::*;

#[derive(Clone)]
struct ValidationMetricsInner {
    pending_verification:   IntGauge,
    verification_wait_time: Histogram,
    eth_transition_updates: Histogram,
    /// different times for different types of orders. this is the total
    /// time including the wait.
    processing_time:        HistogramVec
}

impl Default for ValidationMetricsInner {
    fn default() -> Self {
        let buckets = prometheus::exponential_buckets(1.0, 2.0, 15).unwrap();

        let pending_verification = prometheus::register_int_gauge!(
            "pending_order_verification",
            "the amount of orders, currently in queue to be verified"
        )
        .unwrap();

        let verification_wait_time = prometheus::register_histogram!(
            "verification_wait_time",
            "the amount of time a order spent in the verification queue",
            buckets.clone()
        )
        .unwrap();

        let eth_transition_updates = prometheus::register_histogram!(
            "verification_update_time",
            "How long it takes to handle a new block update",
            buckets.clone()
        )
        .unwrap();

        let processing_time = prometheus::register_histogram_vec!(
            "verification_processing_time",
            "the total processing time of a order based on it's type",
            &["order_type"],
            buckets
        )
        .unwrap();

        Self {
            pending_verification,
            verification_wait_time,
            eth_transition_updates,
            processing_time
        }
    }
}
impl ValidationMetricsInner {
    fn inc_pending(&self) {
        self.pending_verification.inc();
    }

    fn dec_pending(&self) {
        self.pending_verification.dec();
    }

    async fn handle_pending<'a, T>(
        &self,
        f: impl FnOnce() -> Pin<Box<dyn Future<Output = T> + Send + 'a>>
    ) -> T {
        self.inc_pending();
        let start = Instant::now();
        let r = f().await;
        let elapsed = start.elapsed().as_nanos() as f64;
        self.verification_wait_time.observe(elapsed);
        self.dec_pending();

        r
    }
}

#[derive(Clone)]
pub struct ValidationMetrics(Option<ValidationMetricsInner>);

impl ValidationMetrics {
    pub fn new() -> Self {
        Self(
            METRICS_ENABLED
                .get()
                .copied()
                .unwrap_or_default()
                .then(ValidationMetricsInner::default)
        )
    }

    pub async fn measure_wait_time<'a, T>(
        &self,
        f: impl FnOnce() -> Pin<Box<dyn Future<Output = T> + Send + 'a>>
    ) -> T {
        if let Some(inner) = self.0.as_ref() {
            return inner.handle_pending(f).await
        }

        f().await
    }

    pub fn new_order(&self, is_searcher: bool, f: impl FnOnce()) {
        // self.0.as_ref().inspect(|metrics| {}).unwrap_or_else(f);

        let start = Instant::now();
        f();
    }
}
