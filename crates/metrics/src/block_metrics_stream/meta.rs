use std::sync::OnceLock;

use alloy_primitives::Address;

#[derive(Debug, Clone)]
pub struct StreamMetadata {
    pub node_address: Address,
    pub chain_id:     u64
}

static STREAM_METADATA: OnceLock<StreamMetadata> = OnceLock::new();

pub fn initialize_stream_metadata(node_address: Address, chain_id: u64) -> eyre::Result<()> {
    STREAM_METADATA
        .set(StreamMetadata { node_address, chain_id })
        .map_err(|_| eyre::eyre!("block metrics stream metadata already initialized"))
}

pub fn stream_metadata() -> Option<&'static StreamMetadata> {
    STREAM_METADATA.get()
}
