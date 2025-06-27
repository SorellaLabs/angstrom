use std::{
    collections::{HashMap, HashSet},
    sync::Arc
};

use alloy::primitives::Address;
use angstrom_types::{
    block_sync::BlockSyncProducer, contract_payloads::angstrom::AngstromPoolConfigStore,
    primitive::ChainExt
};
use chrono::Utc;
use reth_execution_types::Chain;
use reth_provider::CanonStateNotification;
use serde::{Deserialize, Serialize};
use serde_with::{DisplayFromStr, serde_as};
use telemetry_recorder::OrderTelemetryExt;

use crate::manager::EthDataCleanser;

/// The state of our eth-updater, right before we go to the next block
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EthUpdaterSnapshot {
    pub angstrom_address:  Address,
    pub periphery_address: Address,
    pub chain_update:      AngstromChainUpdate,
    #[serde_as(as = "HashMap<DisplayFromStr, _>")]
    pub angstrom_tokens:   HashMap<Address, usize>,

    pub pool_store: Arc<AngstromPoolConfigStore>,
    /// the set of currently active nodes.
    pub node_set:   HashSet<Address>,
    pub timestamp:  chrono::DateTime<Utc>
}

impl<Sync: BlockSyncProducer> From<(&EthDataCleanser<Sync>, CanonStateNotification)>
    for EthUpdaterSnapshot
{
    fn from((data, update): (&EthDataCleanser<Sync>, CanonStateNotification)) -> Self {
        Self {
            angstrom_tokens:   data.angstrom_tokens.clone(),
            angstrom_address:  data.angstrom_address,
            periphery_address: data.periphery_address,
            chain_update:      update.into(),
            pool_store:        data.pool_store.clone(),
            node_set:          data.node_set.clone(),
            timestamp:         Utc::now()
        }
    }
}

impl OrderTelemetryExt for EthUpdaterSnapshot {
    fn into_message(self) -> telemetry_recorder::TelemetryMessage {
        let block = self.chain_update.get_block_number();

        telemetry_recorder::TelemetryMessage::EthSnapshot {
            blocknum:     block,
            eth_snapshot: serde_json::to_value(self).unwrap()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AngstromChainUpdate {
    New(Arc<Chain>),
    Reorg { new: Arc<Chain>, old: Arc<Chain> }
}

impl AngstromChainUpdate {
    fn get_block_number(&self) -> u64 {
        match self {
            Self::New(n) => n.tip_number(),
            Self::Reorg { new, .. } => new.tip_number()
        }
    }
}

impl From<CanonStateNotification> for AngstromChainUpdate {
    fn from(value: CanonStateNotification) -> Self {
        match value {
            CanonStateNotification::Commit { new } => Self::New(new),
            CanonStateNotification::Reorg { old, new } => Self::Reorg { new, old }
        }
    }
}
