use std::{cmp, sync::Arc, time::Duration};

use eyre::WrapErr;
use tokio::{
    sync::mpsc,
    time::{Instant, MissedTickBehavior}
};
use tokio_postgres::{Client, NoTls, Transaction};

use super::{
    config::BlockMetricsDbConfig,
    event::BlockMetricEvent,
    health::BlockMetricsDbWriterMetrics,
    stats::QueueStats,
    util::{u64_to_i64, u128_to_i64, unix_now, usize_to_i64}
};

const SQL_TRY_ADVISORY_LOCK: &str =
    include_str!("../../sql/block_metrics_db/try_advisory_lock.sql");
const SQL_ADVISORY_UNLOCK: &str = include_str!("../../sql/block_metrics_db/advisory_unlock.sql");
const SQL_DELETE_ENDPOINT_RETENTION: &str =
    include_str!("../../sql/block_metrics_db/delete_endpoint_retention.sql");
const SQL_DELETE_STATE_RETENTION: &str =
    include_str!("../../sql/block_metrics_db/delete_state_retention.sql");
const SQL_DELETE_BASE_RETENTION: &str =
    include_str!("../../sql/block_metrics_db/delete_base_retention.sql");

const SQL_UPSERT_PREPROPOSAL_ORDERS: &str =
    include_str!("../../sql/block_metrics_db/upsert_preproposal_orders.sql");
const SQL_UPSERT_STATE_TRANSITION: &str =
    include_str!("../../sql/block_metrics_db/upsert_state_transition.sql");
const SQL_UPSERT_PREPROPOSALS_COLLECTED: &str =
    include_str!("../../sql/block_metrics_db/upsert_preproposals_collected.sql");
const SQL_UPSERT_IS_LEADER: &str = include_str!("../../sql/block_metrics_db/upsert_is_leader.sql");
const SQL_UPSERT_MATCHING_INPUT_PRE_QUORUM: &str =
    include_str!("../../sql/block_metrics_db/upsert_matching_input_pre_quorum.sql");
const SQL_UPSERT_MATCHING_INPUT_POST_QUORUM: &str =
    include_str!("../../sql/block_metrics_db/upsert_matching_input_post_quorum.sql");
const SQL_UPSERT_MATCHING_RESULTS: &str =
    include_str!("../../sql/block_metrics_db/upsert_matching_results.sql");
const SQL_UPSERT_SUBMISSION_STARTED: &str =
    include_str!("../../sql/block_metrics_db/upsert_submission_started.sql");
const SQL_UPSERT_SUBMISSION_COMPLETED: &str =
    include_str!("../../sql/block_metrics_db/upsert_submission_completed.sql");
const SQL_INSERT_SUBMISSION_ENDPOINT: &str =
    include_str!("../../sql/block_metrics_db/insert_submission_endpoint.sql");
const SQL_UPSERT_BUNDLE_INCLUDED: &str =
    include_str!("../../sql/block_metrics_db/upsert_bundle_included.sql");
const SQL_UPSERT_PROPOSAL_BUILD_TIME: &str =
    include_str!("../../sql/block_metrics_db/upsert_proposal_build_time.sql");
const SQL_UPSERT_PROPOSAL_VERIFICATION_TIME: &str =
    include_str!("../../sql/block_metrics_db/upsert_proposal_verification_time.sql");
const SQL_UPSERT_CONSENSUS_COMPLETION_TIME: &str =
    include_str!("../../sql/block_metrics_db/upsert_consensus_completion_time.sql");

const RETENTION_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const MAX_RECONNECT_BACKOFF: Duration = Duration::from_secs(30);
const INITIAL_RECONNECT_BACKOFF: Duration = Duration::from_millis(250);
const RETENTION_LOCK_ID: i64 = 0x414E475354524F4D;

pub(crate) struct BlockMetricsDbWriter {
    config:                  BlockMetricsDbConfig,
    node_address:            String,
    chain_id:                i64,
    receiver:                mpsc::Receiver<BlockMetricEvent>,
    stats:                   Arc<QueueStats>,
    writer_metrics:          BlockMetricsDbWriterMetrics,
    reconnect_backoff:       Duration,
    next_connect_attempt_at: Instant
}

impl BlockMetricsDbWriter {
    pub(crate) fn new(
        config: BlockMetricsDbConfig,
        node_address: String,
        chain_id: i64,
        receiver: mpsc::Receiver<BlockMetricEvent>,
        stats: Arc<QueueStats>,
        writer_metrics: BlockMetricsDbWriterMetrics
    ) -> Self {
        Self {
            config,
            node_address,
            chain_id,
            receiver,
            stats,
            writer_metrics,
            reconnect_backoff: INITIAL_RECONNECT_BACKOFF,
            next_connect_attempt_at: Instant::now()
        }
    }

    pub(crate) async fn run(mut self) {
        let mut client: Option<Client> = None;
        let mut buffer: Vec<BlockMetricEvent> = Vec::with_capacity(self.config.batch_size);

        let mut flush_tick =
            tokio::time::interval(Duration::from_millis(self.config.flush_interval_ms));
        flush_tick.set_missed_tick_behavior(MissedTickBehavior::Delay);

        let mut retention_tick = tokio::time::interval(RETENTION_INTERVAL);
        retention_tick.set_missed_tick_behavior(MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                maybe_event = self.receiver.recv(), if buffer.len() < self.config.batch_size => {
                    match maybe_event {
                        Some(event) => {
                            let snapshot = self.stats.on_dequeue_to_buffer();
                            self.writer_metrics.set_depths(snapshot);
                            buffer.push(event);
                        }
                        None => {
                            if !buffer.is_empty() {
                                self.flush_buffer(&mut client, &mut buffer).await;
                            }
                            break;
                        }
                    }
                }
                _ = flush_tick.tick() => {
                    if !buffer.is_empty() {
                        self.flush_buffer(&mut client, &mut buffer).await;
                    }
                }
                _ = retention_tick.tick() => {
                    self.run_retention(&mut client).await;
                }
            }

            if buffer.len() >= self.config.batch_size {
                self.flush_buffer(&mut client, &mut buffer).await;
            }
        }
    }

    async fn flush_buffer(
        &mut self,
        client: &mut Option<Client>,
        buffer: &mut Vec<BlockMetricEvent>
    ) {
        self.ensure_client(client).await;
        let Some(db) = client.as_mut() else {
            return;
        };

        let flush_result = flush_batch(db, self.chain_id, &self.node_address, buffer).await;
        if let Err(error) = flush_result {
            tracing::warn!(?error, "failed to flush block metrics batch");
            self.stats.increment_write_failures();
            self.writer_metrics.increment_write_failures();
            *client = None;
            return;
        }

        let flushed_events = buffer.len();
        buffer.clear();
        let snapshot = self.stats.on_flush_success(flushed_events);
        self.writer_metrics.set_depths(snapshot);
        self.stats.increment_flush_success();
        self.writer_metrics.increment_flush_success();

        let unix_ts = unix_now();
        self.stats.set_last_flush_unixtime(unix_ts);
        self.writer_metrics.set_last_flush_unixtime(unix_ts);
    }

    async fn run_retention(&mut self, client: &mut Option<Client>) {
        self.ensure_client(client).await;
        let Some(db) = client.as_mut() else {
            return;
        };

        let lock_row = match db
            .query_one(SQL_TRY_ADVISORY_LOCK, &[&RETENTION_LOCK_ID])
            .await
        {
            Ok(row) => row,
            Err(error) => {
                tracing::warn!(?error, "failed to acquire retention lock");
                self.stats.increment_write_failures();
                self.writer_metrics.increment_write_failures();
                *client = None;
                return;
            }
        };

        let locked: bool = lock_row.get(0);
        if !locked {
            return;
        }

        let retention_days = i32::try_from(self.config.retention_days).unwrap_or(i32::MAX);
        let delete_result = async {
            let deleted_endpoint = db
                .execute(SQL_DELETE_ENDPOINT_RETENTION, &[&retention_days])
                .await?;
            let deleted_state = db
                .execute(SQL_DELETE_STATE_RETENTION, &[&retention_days])
                .await?;
            let deleted_base = db
                .execute(SQL_DELETE_BASE_RETENTION, &[&retention_days])
                .await?;

            Ok::<u64, tokio_postgres::Error>(deleted_endpoint + deleted_state + deleted_base)
        }
        .await;

        let _ = db.execute(SQL_ADVISORY_UNLOCK, &[&RETENTION_LOCK_ID]).await;

        match delete_result {
            Ok(deleted) => {
                self.stats.add_retention_deletes(deleted);
                self.writer_metrics.add_retention_deletes(deleted);
            }
            Err(error) => {
                tracing::warn!(?error, "failed to run block metrics retention");
                self.stats.increment_write_failures();
                self.writer_metrics.increment_write_failures();
                *client = None;
            }
        }
    }

    async fn ensure_client(&mut self, client: &mut Option<Client>) {
        if client.is_some() {
            return;
        }
        if Instant::now() < self.next_connect_attempt_at {
            return;
        }

        match tokio_postgres::connect(&self.config.db_url, NoTls).await {
            Ok((db, connection)) => {
                tokio::spawn(async move {
                    if let Err(error) = connection.await {
                        tracing::warn!(?error, "block metrics db connection task ended");
                    }
                });

                *client = Some(db);
                self.reconnect_backoff = INITIAL_RECONNECT_BACKOFF;
                self.next_connect_attempt_at = Instant::now();
            }
            Err(error) => {
                tracing::warn!(?error, "failed to connect to block metrics db");
                self.stats.increment_write_failures();
                self.writer_metrics.increment_write_failures();
                self.schedule_reconnect();
            }
        }
    }

    fn schedule_reconnect(&mut self) {
        self.next_connect_attempt_at = Instant::now() + self.reconnect_backoff;
        self.reconnect_backoff = cmp::min(
            self.reconnect_backoff
                .checked_mul(2)
                .unwrap_or(MAX_RECONNECT_BACKOFF),
            MAX_RECONNECT_BACKOFF
        );
    }
}

async fn flush_batch(
    db: &mut Client,
    chain_id: i64,
    node_address: &str,
    events: &[BlockMetricEvent]
) -> eyre::Result<()> {
    let tx = db
        .transaction()
        .await
        .wrap_err("failed to open block metrics db transaction")?;

    for event in events {
        apply_event(&tx, chain_id, node_address, event).await?;
    }

    tx.commit()
        .await
        .wrap_err("failed to commit block metrics db transaction")?;

    Ok(())
}

async fn apply_event(
    tx: &Transaction<'_>,
    chain_id: i64,
    node_address: &str,
    event: &BlockMetricEvent
) -> Result<(), tokio_postgres::Error> {
    match event {
        BlockMetricEvent::PreproposalOrders { block, limit, searcher } => {
            tx.execute(
                SQL_UPSERT_PREPROPOSAL_ORDERS,
                &[
                    &chain_id,
                    &u64_to_i64(*block),
                    &node_address,
                    &usize_to_i64(*limit),
                    &usize_to_i64(*searcher)
                ]
            )
            .await?;
        }
        BlockMetricEvent::StateTransition { block, state, slot_offset_ms, limit, searcher } => {
            tx.execute(
                SQL_UPSERT_STATE_TRANSITION,
                &[
                    &chain_id,
                    &u64_to_i64(*block),
                    &node_address,
                    &state,
                    &u64_to_i64(*slot_offset_ms),
                    &usize_to_i64(*limit),
                    &usize_to_i64(*searcher)
                ]
            )
            .await?;
        }
        BlockMetricEvent::PreproposalsCollected { block, count } => {
            tx.execute(
                SQL_UPSERT_PREPROPOSALS_COLLECTED,
                &[&chain_id, &u64_to_i64(*block), &node_address, &usize_to_i64(*count)]
            )
            .await?;
        }
        BlockMetricEvent::IsLeader { block, is_leader } => {
            tx.execute(
                SQL_UPSERT_IS_LEADER,
                &[&chain_id, &u64_to_i64(*block), &node_address, is_leader]
            )
            .await?;
        }
        BlockMetricEvent::MatchingInputPreQuorum { block, limit, searcher } => {
            tx.execute(
                SQL_UPSERT_MATCHING_INPUT_PRE_QUORUM,
                &[
                    &chain_id,
                    &u64_to_i64(*block),
                    &node_address,
                    &usize_to_i64(*limit),
                    &usize_to_i64(*searcher)
                ]
            )
            .await?;
        }
        BlockMetricEvent::MatchingInputPostQuorum { block, limit, searcher } => {
            tx.execute(
                SQL_UPSERT_MATCHING_INPUT_POST_QUORUM,
                &[
                    &chain_id,
                    &u64_to_i64(*block),
                    &node_address,
                    &usize_to_i64(*limit),
                    &usize_to_i64(*searcher)
                ]
            )
            .await?;
        }
        BlockMetricEvent::MatchingResults {
            block,
            pools_solved,
            filled,
            partial,
            unfilled,
            killed,
            bundle_generated
        } => {
            tx.execute(
                SQL_UPSERT_MATCHING_RESULTS,
                &[
                    &chain_id,
                    &u64_to_i64(*block),
                    &node_address,
                    &usize_to_i64(*pools_solved),
                    &usize_to_i64(*filled),
                    &usize_to_i64(*partial),
                    &usize_to_i64(*unfilled),
                    &usize_to_i64(*killed),
                    bundle_generated
                ]
            )
            .await?;
        }
        BlockMetricEvent::SubmissionStarted { block, slot_offset_ms } => {
            tx.execute(
                SQL_UPSERT_SUBMISSION_STARTED,
                &[&chain_id, &u64_to_i64(*block), &node_address, &u64_to_i64(*slot_offset_ms)]
            )
            .await?;
        }
        BlockMetricEvent::SubmissionCompleted { block, slot_offset_ms, latency_ms, success } => {
            tx.execute(
                SQL_UPSERT_SUBMISSION_COMPLETED,
                &[
                    &chain_id,
                    &u64_to_i64(*block),
                    &node_address,
                    &u64_to_i64(*slot_offset_ms),
                    &u64_to_i64(*latency_ms),
                    success
                ]
            )
            .await?;
        }
        BlockMetricEvent::SubmissionEndpoint {
            block,
            submitter_type,
            endpoint,
            success,
            latency_ms
        } => {
            tx.execute(
                SQL_INSERT_SUBMISSION_ENDPOINT,
                &[
                    &chain_id,
                    &u64_to_i64(*block),
                    &node_address,
                    &submitter_type,
                    &endpoint,
                    success,
                    &u64_to_i64(*latency_ms)
                ]
            )
            .await?;
        }
        BlockMetricEvent::BundleIncluded { block, included } => {
            tx.execute(
                SQL_UPSERT_BUNDLE_INCLUDED,
                &[&chain_id, &u64_to_i64(*block), &node_address, included]
            )
            .await?;
        }
        BlockMetricEvent::ProposalBuildTime { block, time_ms } => {
            tx.execute(
                SQL_UPSERT_PROPOSAL_BUILD_TIME,
                &[&chain_id, &u64_to_i64(*block), &node_address, &u128_to_i64(*time_ms)]
            )
            .await?;
        }
        BlockMetricEvent::ProposalVerificationTime { block, time_ms } => {
            tx.execute(
                SQL_UPSERT_PROPOSAL_VERIFICATION_TIME,
                &[&chain_id, &u64_to_i64(*block), &node_address, &u128_to_i64(*time_ms)]
            )
            .await?;
        }
        BlockMetricEvent::ConsensusCompletionTime { block, time_ms } => {
            tx.execute(
                SQL_UPSERT_CONSENSUS_COMPLETION_TIME,
                &[&chain_id, &u64_to_i64(*block), &node_address, &u128_to_i64(*time_ms)]
            )
            .await?;
        }
    }

    Ok(())
}
