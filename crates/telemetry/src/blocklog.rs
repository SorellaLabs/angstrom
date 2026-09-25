use std::{
    collections::{HashMap, VecDeque},
    io::{Read, Write}
};

use alloy_primitives::{FixedBytes, keccak256};
use angstrom_eth::telemetry::EthUpdaterSnapshot;
use angstrom_types::{
    contract_bindings::angstrom::Angstrom::PoolKey, pair_with_price::PairsWithPrice,
    primitive::PoolId, uni_structure::BaselinePoolState
};
use base64::Engine;
use chrono::{DateTime, Utc};
use flate2::Compression;
use order_pool::telemetry::OrderPoolSnapshot;
use serde::{Deserialize, Serialize};
use validation::telemetry::ValidationSnapshot;

use crate::{NodeConstants, TelemetryMessage};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockLog {
    pub blocknum:            u64,
    pub order_pool_snapshot: Option<OrderPoolSnapshot>,
    pub eth_snapshot:        Option<EthUpdaterSnapshot>,
    pub validation_snapshot: Option<ValidationSnapshot>,
    pub constants:           Option<NodeConstants>,
    pub pool_keys:           Option<Vec<PoolKey>>,
    pub pool_snapshots:      Option<HashMap<PoolId, BaselinePoolState>>,
    pub events:              Vec<TelemetryMessage>,
    pub gas_price_snapshot:  Option<(HashMap<PoolId, VecDeque<PairsWithPrice>>, u128)>,
    pub error:               Option<(String, chrono::DateTime<Utc>)>,
    pub backtrace:           Option<String>
}

impl BlockLog {
    pub fn new(blocknum: u64) -> Self {
        Self {
            blocknum,
            constants: None,
            eth_snapshot: None,
            order_pool_snapshot: None,
            validation_snapshot: None,
            pool_keys: None,
            pool_snapshots: None,
            events: Vec::new(),
            gas_price_snapshot: None,
            error: None,
            backtrace: None
        }
    }

    pub fn everything_to_replay(&self) -> bool {
        self.order_pool_snapshot.is_some()
            && self.eth_snapshot.is_some()
            && self.validation_snapshot.is_some()
            && self.constants.is_some()
    }

    pub fn error_unique_id(&self) -> FixedBytes<4> {
        let mut slice = [0u8; 4];
        self.error
            .as_ref()
            .inspect(|e| slice.copy_from_slice(&keccak256(&e.0)[0..4]));

        FixedBytes::<4>::new(slice)
    }

    pub fn has_error(&self) -> bool {
        self.error.is_some()
    }

    pub fn set_orderpool(&mut self, snap: OrderPoolSnapshot) {
        self.order_pool_snapshot = Some(snap);
    }

    pub fn set_eth(&mut self, snap: EthUpdaterSnapshot) {
        self.eth_snapshot = Some(snap);
    }

    pub fn set_validation(&mut self, snap: ValidationSnapshot) {
        self.validation_snapshot = Some(snap);
    }

    /// Produce a copy of this log targetting another block number.  This is for
    /// replay on a local chain
    pub fn at_block(&self, blocknum: u64) -> Self {
        Self { blocknum, ..self.clone() }
    }

    pub fn blocknum(&self) -> u64 {
        self.blocknum
    }

    pub fn events(&self) -> &[TelemetryMessage] {
        &self.events
    }

    pub fn constants(&self) -> Option<&NodeConstants> {
        self.constants.as_ref()
    }

    pub fn pool_keys(&self) -> Option<&Vec<PoolKey>> {
        self.pool_keys.as_ref()
    }

    pub fn gas_price_snapshot(&self) -> Option<&(HashMap<PoolId, VecDeque<PairsWithPrice>>, u128)> {
        self.gas_price_snapshot.as_ref()
    }

    pub fn set_pool_snapshots(&mut self, pool_snapshots: HashMap<PoolId, BaselinePoolState>) {
        self.pool_snapshots = Some(pool_snapshots);
    }

    pub fn set_pool_keys(&mut self, pool_keys: Vec<PoolKey>) {
        self.pool_keys = Some(pool_keys);
    }

    pub fn set_node_constants(&mut self, node_consts: NodeConstants) {
        self.constants = Some(node_consts);
    }

    pub fn set_gas_price_snapshot(
        &mut self,
        snapshot: (HashMap<PoolId, VecDeque<PairsWithPrice>>, u128)
    ) {
        self.gas_price_snapshot = Some(snapshot);
    }

    pub fn add_event(&mut self, event: TelemetryMessage) {
        self.events.push(event)
    }

    pub fn error(&mut self, error: String, timestamp: DateTime<Utc>, backtrace: String) {
        self.error = Some((error, timestamp));
        self.backtrace = Some(backtrace);
    }

    pub fn to_deflate_base64_str(&self) -> String {
        let bytes = serde_json::to_vec(&self).unwrap();
        let mut codec = flate2::write::DeflateEncoder::new(Vec::new(), Compression::default());
        let _ = codec.write_all(&bytes);
        let compressed = codec.finish().unwrap();
        base64::prelude::BASE64_STANDARD.encode(&compressed)
    }

    pub fn from_deflate_base64(data: &[u8]) -> eyre::Result<Self> {
        let bytes = base64::prelude::BASE64_STANDARD.decode(data)?;
        let mut s = vec![];
        flate2::read::DeflateDecoder::new(bytes.as_slice()).read_to_end(&mut s)?;
        Ok(serde_json::from_slice(&s)?)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use alloy_primitives::{Address, B256};
    use angstrom_eth::telemetry::{AngstromChainUpdate, EthUpdaterSnapshot};
    use angstrom_types::contract_payloads::protocol_fees::{DonationSplitSnapshot, DonationSplits};

    use super::BlockLog;

    #[test]
    fn can_compress_and_decompress() {
        // Very basic compress/decompress test
        let log = BlockLog::new(100);
        let compressed = log.to_deflate_base64_str();
        let decompressed = BlockLog::from_deflate_base64(compressed.as_bytes()).unwrap();
        assert_eq!(log.blocknum, decompressed.blocknum, "Blocknum does not match");
    }

    #[test]
    fn a_recording_without_the_protocol_fee_config_loads_with_none() {
        let mut log = BlockLog::new(100);
        log.eth_snapshot = Some(EthUpdaterSnapshot {
            angstrom_address:    Address::ZERO,
            periphery_address:   Address::ZERO,
            chain_update:        AngstromChainUpdate::New(Arc::new(Default::default())),
            angstrom_tokens:     Default::default(),
            pool_store:          Default::default(),
            node_set:            Default::default(),
            protocol_fee_config: Some(DonationSplitSnapshot {
                block_number: 0,
                block_hash:   B256::ZERO,
                splits:       DonationSplits::new(750_000, 1_000_000).unwrap()
            }),
            timestamp:           Default::default()
        });
        let mut json = serde_json::to_value(&log).unwrap();
        json["eth_snapshot"]
            .as_object_mut()
            .unwrap()
            .remove("protocol_fee_config")
            .unwrap();

        let loaded: BlockLog = serde_json::from_value(json).unwrap();

        assert_eq!(loaded.eth_snapshot.unwrap().protocol_fee_config, None);
    }
}
