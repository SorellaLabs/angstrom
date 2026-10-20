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
    /// Starts at the configured minimum, including after a restart; each round
    /// adds its own jitter.
    pub fn new(config: ConsensusTimingConfig) -> Self {
        Self {
            wait_duration: config.min_wait_time_ms(),
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
    use futures::FutureExt;

    use super::*;

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

    #[tokio::test(start_paused = true)]
    async fn a_restarted_node_starts_at_the_minimum_wait() {
        let config =
            ConsensusTimingConfig { min_wait_duration_ms: 9_000, max_wait_duration_ms: 11_200 };
        let active = PreProposalWaitTrigger::new(config).update_for_new_round(None, Duration::ZERO);
        assert!(
            (Duration::from_millis(9_003)..=Duration::from_millis(9_040))
                .contains(&active.wait_duration),
            "{:?}",
            active.wait_duration
        );
    }
}
