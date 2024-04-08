use std::{
    pin::Pin,
    task::{Context, Poll},
    time::Duration
};

use angstrom_types::submission::BestBundles;
use angstrom_utils::{ConsensusState, IsLeader, PRE_PROPOSE};
use futures::FutureExt;

use super::{
    pre_propose::PreProposeState, GlobalStateContext, RoundAction, RoundStateMessage,
    StateTransition, Timeout
};

/// During this State. Everyone continuously is updating there best bundles
/// collecting up until the timeout occurs
#[allow(dead_code)]
pub struct OrderAccumulationState {
    timeout:     Timeout,
    best_bundle: Option<BestBundles>,
    is_leader:   IsLeader
}
impl OrderAccumulationState {
    pub fn new(timeout: Timeout, is_leader: IsLeader) -> Self {
        Self { timeout, best_bundle: None, is_leader }
    }

    #[allow(dead_code)]
    pub fn new_bundle(&mut self, bundle: BestBundles) {
        if self
            .best_bundle
            .as_ref()
            .map(|cur_best| bundle.get_weight() > cur_best.get_weight())
            .filter(|f| *f)
            .is_some()
        {
            self.best_bundle.replace(bundle);
        }
    }
}

impl StateTransition for OrderAccumulationState {
    fn should_transition(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        _: GlobalStateContext
    ) -> Poll<(RoundAction, ConsensusState, Option<RoundStateMessage>)> {
        self.timeout.poll_unpin(cx).map(|_| {
            (
                RoundAction::PrePropose(PreProposeState::new(
                    // TODO: placeholder
                    Timeout::new(Duration::from_secs(6)),
                    self.best_bundle.take().unwrap()
                )),
                PRE_PROPOSE,
                // TODO: this will be there pre pose commit
                Some(RoundStateMessage::PrePropose())
            )
        })
    }
}
