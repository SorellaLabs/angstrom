pub mod simulations;
pub mod state;
use prometheus::{Histogram, HistogramVec, IntGauge};
pub use simulations::*;
pub use state::*;

#[derive(Clone)]
pub struct ValidationMetrics {
    pending_verification:   IntGauge,
    verification_wait_time: Histogram,
    eth_transition_updates: Histogram,
    processing_time:        HistogramVec
}
