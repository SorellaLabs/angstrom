#[derive(Debug, Clone)]
pub struct BlockMetricsDbConfig {
    pub db_url:            String,
    pub queue_capacity:    usize,
    pub batch_size:        usize,
    pub flush_interval_ms: u64,
    pub retention_days:    i64
}

impl BlockMetricsDbConfig {
    pub(crate) fn validate(&self) -> eyre::Result<()> {
        if self.db_url.trim().is_empty() {
            eyre::bail!("block metrics db url cannot be empty");
        }
        if self.queue_capacity == 0 {
            eyre::bail!("block metrics db queue capacity must be greater than zero");
        }
        if self.batch_size == 0 {
            eyre::bail!("block metrics db batch size must be greater than zero");
        }
        if self.flush_interval_ms == 0 {
            eyre::bail!("block metrics db flush interval must be greater than zero");
        }
        if self.retention_days <= 0 {
            eyre::bail!("block metrics db retention days must be greater than zero");
        }

        Ok(())
    }
}
