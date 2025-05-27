use std::{
    collections::HashMap,
    io::{Read, Write}
};

use angstrom_types::{
    contract_bindings::angstrom::Angstrom::PoolKey, primitive::PoolId,
    uni_structure::BaselinePoolState
};
use base64::Engine;
use flate2::Compression;
use serde::{Deserialize, Serialize};

use crate::{NodeConstants, TelemetryMessage};

#[derive(Serialize, Deserialize)]
pub struct BlockLog {
    blocknum:       u64,
    constants:      Option<NodeConstants>,
    pool_keys:      Option<Vec<PoolKey>>,
    pool_snapshots: Option<HashMap<PoolId, BaselinePoolState>>,
    events:         Vec<TelemetryMessage>,
    error:          Option<String>
}

impl BlockLog {
    pub fn new(blocknum: u64) -> Self {
        Self {
            blocknum,
            constants: None,
            pool_keys: None,
            pool_snapshots: None,
            events: Vec::new(),
            error: None
        }
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

    pub fn set_pool_snapshots(&mut self, pool_snapshots: HashMap<PoolId, BaselinePoolState>) {
        self.pool_snapshots = Some(pool_snapshots);
    }

    pub fn set_pool_keys(&mut self, pool_keys: Vec<PoolKey>) {
        self.pool_keys = Some(pool_keys);
    }

    pub fn set_node_constants(&mut self, node_consts: NodeConstants) {
        self.constants = Some(node_consts);
    }

    pub fn add_event(&mut self, event: TelemetryMessage) {
        self.events.push(event)
    }

    pub fn error(&mut self, error: String) {
        self.error = Some(error)
    }

    pub fn to_deflate_base64_str(&self) -> String {
        let json = serde_json::to_string(self).unwrap();
        let mut codec = flate2::write::DeflateEncoder::new(Vec::new(), Compression::default());
        let _ = codec.write_all(json.as_bytes());
        let compressed = codec.finish().unwrap();
        base64::prelude::BASE64_STANDARD.encode(&compressed)
    }

    pub fn from_deflate_base64_str(string: &str) -> Self {
        let bytes = base64::prelude::BASE64_STANDARD.decode(string).unwrap();
        let mut codec = flate2::read::DeflateDecoder::new(bytes.as_slice());
        let mut s = String::new();
        let _ = codec.read_to_string(&mut s);
        let blocklog: BlockLog = serde_json::from_str(&s).unwrap();
        blocklog
    }
}

#[cfg(test)]
mod tests {
    use super::BlockLog;

    #[test]
    fn can_compress_and_decompress() {
        // Very basic compress/decompress test
        let log = BlockLog::new(100);
        let compressed = log.to_deflate_base64_str();
        let decompressed = BlockLog::from_deflate_base64_str(&compressed);
        assert_eq!(log.blocknum, decompressed.blocknum, "Blocknum does not match");
    }
}
