use std::{future::Future, task::Poll, time::Duration};

use rand::Rng;
use tokio::time::{Instant, Interval, interval};

use crate::ConsensusTimingConfig;

/// Poll the cutoff committed when the round starts.
const CHECK_INTERVAL: Duration = Duration::from_millis(1);
/// How close we want to be to the creation of the ethereum block
const TARGET_SUBMISSION_TIME_POST_MAX: Duration = Duration::from_millis(200);
/// The amount of the difference we scale by to reach
const SCALING_REM_ADJUSTMENT: u32 = 10;

/// When we should trigger to build our pre-proposals
/// this is very important for maximizing how long we can
/// wait till we start the next block. This helps us maximize
/// the amount of orders we clear while making sure that we
/// never miss a possible slot.
#[derive(Debug)]
pub struct PreProposalWaitTrigger {
    /// Fixed for an active round; only the template is adjusted between rounds.
    wait_duration:  Duration,
    /// the start instant
    start_instant:  Instant,
    /// Waker
    check_interval: Interval,
    config:         ConsensusTimingConfig
}

impl Clone for PreProposalWaitTrigger {
    fn clone(&self) -> Self {
        let mut rng = rand::rng();
        let jitter = Duration::from_millis(rng.random_range(3..=40));
        Self {
            wait_duration:  self.wait_duration + jitter,
            start_instant:  Instant::now(),
            check_interval: interval(CHECK_INTERVAL),
            config:         self.config
        }
    }
}

impl PreProposalWaitTrigger {
    pub fn new(config: ConsensusTimingConfig) -> Self {
        let mut rng = rand::rng();
        let jitter = Duration::from_millis(rng.random_range(3..=40));

        Self {
            wait_duration: config.default_duration() + jitter,
            start_instant: Instant::now(),
            check_interval: interval(CHECK_INTERVAL),
            config
        }
    }

    pub fn update_for_new_round(
        &mut self,
        info: Option<LastRoundInfo>,
        slot_elapsed_time: Duration
    ) -> Self {
        if let Some(info) = info {
            self.update_wait_duration_base(info);
        }

        let mut this = self.clone();
        this.wait_duration = this.wait_duration.saturating_sub(slot_elapsed_time);

        this
    }

    pub fn reset_before_submission(&mut self) {
        self.wait_duration = self
            .wait_duration
            .saturating_sub(TARGET_SUBMISSION_TIME_POST_MAX);
    }

    fn update_wait_duration_base(&mut self, info: LastRoundInfo) {
        let base = self.config.max_wait_time_ms() + TARGET_SUBMISSION_TIME_POST_MAX;

        let delta = if info.time_to_complete < base {
            (base - info.time_to_complete) / SCALING_REM_ADJUSTMENT
        } else {
            (info.time_to_complete - base) * SCALING_REM_ADJUSTMENT
        };

        if self.wait_duration < base {
            self.wait_duration += delta;
        } else {
            self.wait_duration = self.wait_duration.saturating_sub(delta);
        }

        self.wait_duration = self.sigmoid_clamp(self.wait_duration);

        let millis = self.wait_duration.as_millis();
        tracing::info!(trigger = millis, "Updated wait duration to trigger building");
    }

    /// Sigmoid function to clamp the wait time between [`MIN_WAIT_DURATION`]
    /// and [`MAX_WAIT_DURATION`].
    #[inline(always)]
    fn sigmoid_clamp(&self, x: Duration) -> Duration {
        let x_o = self.config.min_wait_time_ms().as_secs_f64()
            + self.config.max_wait_time_ms().as_secs_f64() / 2.0;
        const K: f64 = 1.5; // Steepness of sigmoid

        let x_secs = x.as_secs_f64();
        let adjusted = self.config.min_wait_time_ms().as_secs_f64()
            + ((self.config.max_wait_time_ms() - self.config.min_wait_time_ms()).as_secs_f64())
                / (1.0 + (-K * (x_secs - x_o)).exp());

        // safety lol
        let adjusted = adjusted.clamp(
            self.config.min_wait_time_ms().as_secs_f64(),
            self.config.max_wait_time_ms().as_secs_f64()
        );

        Duration::from_secs_f64(adjusted)
    }
}

/// Incoming orders cannot change an active round's cutoff.
impl Future for PreProposalWaitTrigger {
    type Output = ();

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>
    ) -> std::task::Poll<Self::Output> {
        while self.check_interval.poll_tick(cx).is_ready() {
            if Instant::now().duration_since(self.start_instant) > self.wait_duration {
                return Poll::Ready(());
            }
        }

        Poll::Pending
    }
}

/// Prior-round feedback for the next round's cutoff.
#[derive(Debug)]
pub struct LastRoundInfo {
    /// Round start through matching completion, before encoding and submission.
    pub time_to_complete: Duration
}

#[cfg(test)]
mod tests {
    use alloy::primitives::B256;
    use angstrom_types::{
        orders::OrderId,
        sol_bindings::{grouped_orders::OrderWithStorageData, rpc_orders::TopOfBlockOrder}
    };
    use futures::FutureExt;
    use order_pool::{PoolConfig, order_storage::OrderStorage};

    use super::*;

    #[tokio::test(start_paused = true)]
    async fn incoming_orders_do_not_advance_the_active_cutoff() {
        let pool_id = B256::ZERO;
        let storage = OrderStorage::new(&PoolConfig::with_pool_ids(vec![pool_id]));
        let mut trigger = PreProposalWaitTrigger::new(ConsensusTimingConfig::default());
        trigger.wait_duration = Duration::from_secs(9);
        tokio::time::advance(Duration::from_millis(8_500)).await;
        assert!(
            trigger
                .poll_unpin(&mut std::task::Context::from_waker(futures::task::noop_waker_ref()))
                .is_pending()
        );

        for i in 1..=100u8 {
            let order = OrderWithStorageData::<TopOfBlockOrder> {
                pool_id,
                order_id: OrderId { hash: B256::from([i; 32]), ..Default::default() },
                ..Default::default()
            };
            storage.add_new_searcher_order(order).unwrap();
        }
        assert_eq!(storage.get_all_orders().total_orders(), 100);
        tokio::time::advance(Duration::from_millis(1)).await;
        assert!(
            trigger
                .poll_unpin(&mut std::task::Context::from_waker(futures::task::noop_waker_ref()))
                .is_pending()
        );
        tokio::time::advance(Duration::from_millis(500)).await;
        assert!(
            trigger
                .poll_unpin(&mut std::task::Context::from_waker(futures::task::noop_waker_ref()))
                .is_ready()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn next_round_feedback_cannot_change_an_active_round() {
        let mut template = PreProposalWaitTrigger::new(ConsensusTimingConfig::default());
        let active = template.update_for_new_round(None, Duration::from_secs(2));
        let committed = active.start_instant + active.wait_duration;
        let initial_base = template.wait_duration;
        assert!(
            active.wait_duration
                >= initial_base + Duration::from_millis(3) - Duration::from_secs(2)
        );
        assert!(
            active.wait_duration
                <= initial_base + Duration::from_millis(40) - Duration::from_secs(2)
        );

        template.reset_before_submission();
        let _next = template.update_for_new_round(
            Some(LastRoundInfo { time_to_complete: Duration::from_secs(12) }),
            Duration::from_secs(1)
        );
        assert_eq!(active.start_instant + active.wait_duration, committed);
    }

    #[tokio::test(start_paused = true)]
    async fn a_round_started_after_its_budget_is_due_immediately() {
        let mut template = PreProposalWaitTrigger::new(ConsensusTimingConfig::default());
        let mut active = template.update_for_new_round(None, Duration::from_secs(20));
        tokio::time::advance(Duration::from_millis(1)).await;
        assert!(
            active
                .poll_unpin(&mut std::task::Context::from_waker(futures::task::noop_waker_ref()))
                .is_ready()
        );
    }
}
