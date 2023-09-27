use std::task::Context;

use guard_types::consensus::Time;

/// the representation of these represents time in seconds
/// for the current cutoff period
pub enum ConsensusStage {
    DataPropagation     = 0,
    BundleSigningCutoff = 8,
    /// TODO: not sure if we need this or not. we will leave it
    /// in for now tho
    Bundle23PropCutoff  = 9,
    LeaderProposeCutoff = 10
}

impl ConsensusStage {
    pub fn get_current_period(time: Time) -> Self {
        todo!()
    }
}

pub struct Stage {
    height: u64,
    round:  u64,

    time:  Time,
    stage: ConsensusStage
}

impl Stage {
    pub fn new_height(&mut self) {
        self.round = 0;
        self.height += 1;
    }

    pub fn new_round(&mut self) {
        self.round += 1;
    }

    pub fn round(&self) -> u64 {
        self.round
    }

    pub fn height(&self) -> u64 {
        self.height
    }

    pub fn is_past_proposal_cutoff(&self) -> bool {
        todo!()
    }

    pub fn is_past_proposal_vote_cutoff(&self) -> bool {
        todo!()
    }

    pub fn is_past_bundle_signing_cutoff(&self) -> bool {
        todo!()
    }

    pub fn is_past_bundle23_prop_cutoff(&self) -> bool {
        todo!()
    }

    pub fn update_current_stage(&mut self) {}
}
