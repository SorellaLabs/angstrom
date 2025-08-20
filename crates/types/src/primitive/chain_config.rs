use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ChainConfig {
    pub block_time:                  Duration,
    pub max_order_delay_propagation: u64
}

impl ChainConfig {
    pub fn ethereum() -> Self {
        Self {
            block_time:                  Duration::from_secs(12),
            max_order_delay_propagation: 7000
        }
    }

    pub fn op_angstrom() -> Self {
        Self { block_time: Duration::from_secs(2), max_order_delay_propagation: 0 }
    }
}