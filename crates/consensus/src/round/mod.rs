pub mod bundle;
pub mod leader;
pub mod stage;

pub use bundle::*;
pub use leader::*;
pub use stage::*;

pub struct RoundState {
    stage:            Stage,
    bundle:           BundleVoteManager,
    proposal_manager: ProposalManager
}

impl RoundState {
    pub fn stage(&mut self) -> &mut Stage {
        &mut self.stage
    }

    pub fn bundle(&mut self) -> &mut BundleVoteManager {
        &mut self.bundle
    }

    pub fn proposal_manager(&mut self) -> &mut ProposalManager {
        &mut self.proposal_manager
    }
}
