use std::sync::atomic::{AtomicU64, Ordering};

use alloy::{
    eips::BlockNumberOrTag,
    primitives::Log,
    rpc::types::{Filter, FilterBlockOption}
};
use futures_util::StreamExt;
use reth_provider::CanonStateNotification;
use tokio::sync::{broadcast, RwLock};

use crate::uniswap::{pool_manager::PoolManagerError, pool_providers::PoolManagerProvider};

pub struct CanonicalStateAdapter {
    canon_state_notifications: broadcast::Receiver<CanonStateNotification>,
    last_logs:                 RwLock<Vec<Log>>,
    last_block_number:         AtomicU64
}

impl CanonicalStateAdapter {
    pub fn new(canon_state_notifications: broadcast::Receiver<CanonStateNotification>) -> Self {
        Self {
            canon_state_notifications,
            last_logs: RwLock::new(Vec::new()),
            last_block_number: AtomicU64::new(0)
        }
    }
}

impl PoolManagerProvider for CanonicalStateAdapter {
    fn subscribe_blocks(&self) -> futures::stream::BoxStream<Option<u64>> {
        futures_util::stream::unfold(
            self.canon_state_notifications.resubscribe(),
            move |mut notifications| async move {
                if let Ok(notification) = notifications.recv().await {
                    let mut last_log_write = self.last_logs.write().await;
                    let block = match notification {
                        CanonStateNotification::Commit { new }
                        | CanonStateNotification::Reorg { new, .. } => {
                            let block = new.tip();
                            let logs: Vec<Log> = new
                                .execution_outcome()
                                .logs(block.number)
                                .map_or_else(Vec::new, |logs| logs.cloned().collect());
                            *last_log_write = logs;
                            self.last_block_number.store(block.number, Ordering::SeqCst);
                            Some(Some(block.number))
                        }
                    };
                    Some((block, notifications))
                } else {
                    None
                }
            }
        )
        .filter_map(futures_util::future::ready)
        .boxed()
    }

    async fn get_logs(&self, filter: &Filter) -> Result<Vec<Log>, PoolManagerError> {
        self.validate_filter(filter)?;

        let cache = self.last_logs.read().await;
        let res = cache
            .iter()
            .filter(|log| Self::log_matches_filter(log, filter))
            .cloned()
            .collect();

        Ok(res)
    }
}

impl CanonicalStateAdapter {
    fn validate_filter(&self, filter: &Filter) -> Result<(), PoolManagerError> {
        let last_block = self.last_block_number.load(Ordering::SeqCst);
        if let FilterBlockOption::Range { from_block, to_block } = &filter.block_option {
            let from_equal_block_range = from_block.as_ref().map_or(false, |from| {
                matches!(from, BlockNumberOrTag::Number(from_num)
                    if last_block == *from_num
                )
            });
            let to_equal_to_block_range = to_block.as_ref().map_or(false, |to| {
                matches!(to, BlockNumberOrTag::Number(to_num)
                    if last_block == *to_num
                )
            });

            if !from_equal_block_range || !to_equal_to_block_range {
                return Err(PoolManagerError::InvalidBlockRange);
            }
        }
        Ok(())
    }

    fn log_matches_filter(log: &Log, filter: &Filter) -> bool {
        filter.address.matches(&log.address)
            && filter.topics.iter().enumerate().any(|(i, topic)| {
                topic.matches(
                    log.topics()
                        .get(i)
                        .unwrap_or(&alloy::primitives::B256::ZERO)
                )
            })
    }
}
