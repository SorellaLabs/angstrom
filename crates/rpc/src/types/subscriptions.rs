use std::sync::Arc;

use alloy_primitives::B256;
use angstrom_types::{
    consensus::*, primitive::Angstrom::PoolKey, sol_bindings::grouped_orders::AllOrders
};
use serde::{Deserialize, Serialize};

use super::quoting::{Depth25, Depth5, BBO};

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
    Proposal,
    /// Sends commits when received
    Commits
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub enum ConsensusSubscriptionResult {
    /// Preprosal
    PreProposal(Arc<PreProposal>),
    Proposal(Arc<Proposal>),
    Commits(Arc<Commit>)
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub enum OrderSubscriptionKind {
    /// Any new orders
    NewOrders,
    /// Any new filled orders
    FilledOrders,
    /// Any new reorged orders
    UnfilleOrders,
    /// Any new cancelled orders
    CancelledOrders
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub enum OrderSubscriptionResult {
    NewOrder(AllOrders),
    FilledOrder((u64, AllOrders)),
    UnfilledOrder(AllOrders),
    CancelledOrder(B256)
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub enum QuotingSubscriptionParam {
    #[default]
    None,
    Pool(PoolKey)
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub enum QuotingSubscriptionKind {
    /// The BBO of a given pool
    BBO,
    /// A 5 tick depth on a given pool
    Depth5,
    /// A 25 Tick depth on a given pool
    Depth25
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub enum QuotingSubscriptionResult {
    BBO(Arc<BBO>),
    Depth5(Arc<Depth5>),
    Depth25(Arc<Depth25>)
}
