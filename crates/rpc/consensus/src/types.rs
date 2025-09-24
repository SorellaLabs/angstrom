use std::sync::Arc;

use alloy_primitives::{Address, B256, FixedBytes};
use angstrom_types::{consensus::*, sol_bindings::grouped_orders::AllOrders};
use serde::{Deserialize, Serialize};
use strum::{EnumIter, IntoEnumIterator};

#[derive(Debug, PartialEq, Eq, Hash, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub enum ConsensusSubscriptionKind {
    /// Sends a pre-proposal upon receiving it
    PreProposal,
    /// Send a pre-proposal upon receiving it, but only if it is better than the
    /// current best
    NewBestPreProposal,
    /// Sends the proposal upon receiving it from the proposer
    Proposal
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub enum ConsensusSubscriptionResult {
    /// Preprosal
    PreProposal(Arc<PreProposal>),
    Proposal(Arc<Proposal>)
}
