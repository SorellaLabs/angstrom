use angstrom_pools::AngstromPools;
use index_to_address::AssetIndexToAddress;

pub mod angstrom_pools;
pub mod index_to_address;

/// keeps track of all valid pools and the mappings of asset id to pool id
pub struct AngstromPoolsTracker {
    pub asset_index_to_address: AssetIndexToAddress,
    pub pools:                  AngstromPools
}
