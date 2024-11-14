use std::sync::Arc;

use alloy_primitives::{Address, FixedBytes, B256};
use angstrom_types::{
    consensus::*, contract_bindings::angstrom::Angstrom::PoolKey,
    sol_bindings::grouped_orders::AllOrders
};
use serde::{Deserialize, Serialize};

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

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

#[derive(
    Debug, Default, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub enum OrderSubscriptionFilter {
    /// only returns subscription updates on a singluar pair
    ByPair(FixedBytes<32>),
    /// only returns subscription updates related to a address
    ByAddress(Address),
    /// returns all subscription updates
    #[default]
    None
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub enum OrderSubscriptionResult {
    NewOrder(AllOrders),
    FilledOrder(u64, AllOrders),
    UnfilledOrder(AllOrders),
    CancelledOrder(B256)
}
