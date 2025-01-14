use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, RwLock
};

use alloy::{
    eips::BlockNumberOrTag,
    primitives::Log,
    providers::Provider,
    rpc::types::{Filter, FilterBlockOption}
};
use futures_util::StreamExt;
use reth_provider::CanonStateNotification;
use tokio::sync::broadcast;

use super::PoolMangerBlocks;
use crate::uniswap::{pool_manager::PoolManagerError, pool_providers::PoolManagerProvider};

pub struct CanonicalStateAdapter<P>
where
    P: Provider + 'static
{
    canon_state_notifications: broadcast::Receiver<CanonStateNotification>,
    last_logs:                 Arc<RwLock<Vec<Log>>>,
    last_block_number:         Arc<AtomicU64>,
    node_provider:             Arc<P>
}

impl<P> Clone for CanonicalStateAdapter<P>
where
    P: Provider + 'static
{
    fn clone(&self) -> Self {
        Self {
            canon_state_notifications: self.canon_state_notifications.resubscribe(),
            last_logs:                 self.last_logs.clone(),
            last_block_number:         self.last_block_number.clone(),
            node_provider:             self.node_provider.clone()
        }
    }
}

impl<P> CanonicalStateAdapter<P>
where
    P: Provider + 'static
{
    pub fn new(
        canon_state_notifications: broadcast::Receiver<CanonStateNotification>,
        node_provider: Arc<P>,
        block_number: u64
    ) -> Self {
        Self {
            canon_state_notifications,
            last_logs: Arc::new(RwLock::new(Vec::new())),
            last_block_number: Arc::new(AtomicU64::new(block_number)),
            node_provider
        }
    }
}

impl<P> PoolManagerProvider for CanonicalStateAdapter<P>
where
    P: Provider + 'static
{
    fn provider(&self) -> Arc<impl Provider> {
        self.node_provider.clone()
    }

    fn subscribe_blocks(self) -> futures::stream::BoxStream<'static, Option<PoolMangerBlocks>> {
        futures_util::stream::unfold(
            self.canon_state_notifications.resubscribe(),
            move |mut notifications| {
                let this = self.clone();
                async move {
                    if let Ok(notification) = notifications.recv().await {
                        let mut last_log_write = this.last_logs.write().unwrap();
                        let block = match notification {
                            CanonStateNotification::Commit { new } => {
                                let block = new.tip();

                                let logs: Vec<Log> = new
                                    .execution_outcome()
                                    .logs(block.number)
                                    .map_or_else(Vec::new, |logs| logs.cloned().collect());
                                *last_log_write = logs;
                                this.last_block_number.store(block.number, Ordering::SeqCst);
                                tracing::info!(?block.number,"updated number");
                                Some(Some(PoolMangerBlocks::NewBlock(block.block.number)))
                            }
                            CanonStateNotification::Reorg { old, new } => {
                                let tip = new.tip().block.number;
                                // search 30 blocks back;
                                let start = tip - 30;

                                let range = old
                                    .blocks_iter()
                                    .filter(|b| b.block.number >= start)
                                    .zip(new.blocks_iter().filter(|b| b.block.number >= start))
                                    .filter(|&(old, new)| (old.block.hash() != new.block.hash()))
                                    .map(|(_, new)| new.block.number)
                                    .collect::<Vec<_>>();

                                let range = match range.len() {
                                    0 => tip..=tip,
                                    _ => {
                                        let start = *range.first().unwrap();
                                        let end = *range.last().unwrap();
                                        start..=end
                                    }
                                };

                                let block = new.tip();
                                let mut logs = Vec::new();

                                for block in range.clone() {
                                    logs.extend(
                                        new.execution_outcome()
                                            .logs(block)
                                            .map_or_else(Vec::new, |logs| logs.cloned().collect())
                                    );
                                }

                                *last_log_write = logs;
                                this.last_block_number.store(block.number, Ordering::SeqCst);
                                tracing::info!(?block.number,"updated number");
                                Some(Some(PoolMangerBlocks::Reorg(block.number, range)))
                            }
                        };
                        Some((block, notifications))
                    } else {
                        None
                    }
                }
            }
        )
        .filter_map(futures_util::future::ready)
        .boxed()
    }

    fn get_logs(&self, filter: &Filter) -> Result<Vec<Log>, PoolManagerError> {
        self.validate_filter(filter)?;

        let cache = self.last_logs.read().unwrap();
        let res = cache
            .iter()
            .filter(|log| Self::log_matches_filter(log, filter))
            .cloned()
            .collect();

        Ok(res)
    }
}

impl<P> CanonicalStateAdapter<P>
where
    P: Provider + 'static
{
    fn validate_filter(&self, filter: &Filter) -> Result<(), PoolManagerError> {
        let last_block = self.last_block_number.load(Ordering::SeqCst);
        if let FilterBlockOption::Range { from_block, to_block } = &filter.block_option {
            tracing::debug!(?from_block, ?to_block, ?last_block);

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

            if !(from_equal_block_range && to_equal_to_block_range) {
                return Err(PoolManagerError::InvalidBlockRange)
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
