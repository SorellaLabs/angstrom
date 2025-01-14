use std::{
    future::Future,
    sync::Arc,
    task::Poll,
    time::{Duration, Instant}
};

use tokio::time::{interval, Interval};

use crate::rounds::OrderStorage;

/// How soon we send our pre-proposal
const DEFAULT_DURATION: Duration = Duration::from_secs(9);
/// The frequency we adjust our duration estimate. we have it super frequent
/// because its very low overhead to check
const CHECK_INTERVAL: Duration = Duration::from_millis(1);
/// How much to scale per order in the order pool
const ORDER_SCALING: Duration = Duration::from_millis(10);
/// How close we want to be to the creation of the ethereum block
const TARGET_SUBMISSION_TIME_REM: Duration = Duration::from_millis(800);
/// Eth block time
const ETH_BLOCK_TIME: Duration = Duration::from_secs(12);
/// The amount of the difference we scale by to reach
const SCALING_REM_ADJUSTMENT: u32 = 3;

/// When we should trigger to build our pre-proposals
/// this is very important for maximizing how long we can
/// wait till we start the next block. This helps us maximize
/// the amount of orders we clear while making sure that we
/// never miss a possible slot.
#[derive(Debug)]
pub struct PreProposalWaitTrigger {
    /// the base wait duration that we scale down based on orders.
    wait_duration:  Duration,
    /// the start instant
    start_instant:  Instant,
    /// to track our scaling
    order_storage:  Arc<OrderStorage>,
    /// Waker
    check_interval: Interval
}

impl Clone for PreProposalWaitTrigger {
    fn clone(&self) -> Self {
        Self {
            wait_duration:  self.wait_duration,
            start_instant:  Instant::now(),
            order_storage:  self.order_storage.clone(),
            check_interval: interval(CHECK_INTERVAL)
        }
    }
}

impl PreProposalWaitTrigger {
    pub fn new(order_storage: Arc<OrderStorage>) -> Self {
        Self {
            wait_duration: DEFAULT_DURATION,
            order_storage,
            start_instant: Instant::now(),
            check_interval: interval(CHECK_INTERVAL)
        }
    }

    pub fn update_for_new_round(&mut self, info: Option<LastRoundInfo>) -> Self {
        if let Some(info) = info {
            self.update_wait_duration_base(info);
        }

        self.clone()
    }

    pub fn reset_before_submission(&mut self) {
        self.wait_duration = self
            .wait_duration
            .saturating_sub(TARGET_SUBMISSION_TIME_REM);
    }

    fn update_wait_duration_base(&mut self, info: LastRoundInfo) {
        let base = ETH_BLOCK_TIME - TARGET_SUBMISSION_TIME_REM;

        if info.time_to_complete < base && self.wait_duration < base {
            // if we overestimated the time, we will push our trigger back
            self.wait_duration += (base - info.time_to_complete) / SCALING_REM_ADJUSTMENT;
        } else {
            // otherwise if we underestimated, we will move back
            self.wait_duration -= (info.time_to_complete - base) * SCALING_REM_ADJUSTMENT;
        }

        let mills = self.wait_duration.as_millis();
        tracing::info!(
            trigger = mills,
            "updated wait duration to trigger building!!!!!!!!!!!!!!!!!!!!!!!!!!!"
        );
    }
}

/// Resolves once the scaling has past the start
impl Future for PreProposalWaitTrigger {
    type Output = ();

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>
    ) -> std::task::Poll<Self::Output> {
        while self.check_interval.poll_tick(cx).is_ready() {
            let order_cnt = self.order_storage.get_all_orders().total_orders();

            let target_resolve = self
                .wait_duration
                .saturating_sub(ORDER_SCALING * order_cnt as u32);

            if Instant::now().duration_since(self.start_instant) > target_resolve {
                return Poll::Ready(())
            }
        }

        Poll::Pending
    }
}

/// Details on how to adjust our duration,
/// Given that angstroms matching engine is linear time.
/// we scale the timeout estimation linearly.
#[derive(Debug)]
pub struct LastRoundInfo {
    /// the start of the round to submitting the bundle
    pub time_to_complete: Duration
}
