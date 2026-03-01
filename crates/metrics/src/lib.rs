mod exporter;
use std::sync::OnceLock;

pub use exporter::*;

pub mod block_metrics_stream;

mod bundle_building;

pub mod validation;

mod order_pool;
pub use order_pool::*;

mod consensus;
pub use consensus::*;

mod block_metrics;
pub use block_metrics::*;

mod stream_source;
pub use stream_source::*;

pub static METRICS_ENABLED: OnceLock<bool> = OnceLock::new();
