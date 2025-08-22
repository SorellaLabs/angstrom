use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ChainConfig {
    pub block_time:                  Duration,
    pub max_order_delay_propagation: u64
}

impl ChainConfig {
    pub fn ethereum(block_time: Duration) -> Self {
        Self { block_time, max_order_delay_propagation: 7000 }
    }

    pub fn op_angstrom(block_time: Duration) -> Self {
        Self { block_time, max_order_delay_propagation: 0 }
    }
}
