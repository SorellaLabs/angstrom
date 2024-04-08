use std::sync::Arc;

use angstrom_types::{consensus::*, primitive::Angstrom::PoolKey, rpc::SignedLimitOrder};
use serde::{Deserialize, Serialize};

use super::quoting::{Depth25, Depth5, BBO};

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Hash, Clone)]
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

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub enum ConsensusSubscriptionResult {
    /// Preprosal
    PreProposal(Arc<PreProposal>),
    Proposal(Arc<Proposal>),
    Commits(Arc<Commit>)
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub enum OrderSubscriptionKind {
    /// Any new user order
    NewUserOrder,
    /// Any new searcher order
    NewSearcherOrder,
    /// Only new best searcher orders
    NewBestSearcherOrder,
    /// new transient limit orders
    NewTransientLimitOrders
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub enum OrderSubscriptionResult {
    Order(Arc<SignedLimitOrder>)
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
