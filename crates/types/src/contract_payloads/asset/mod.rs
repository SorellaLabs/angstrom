pub mod builder;
pub mod state;

use std::collections::HashMap;

use alloy::primitives::Address;

use super::Asset;

/// Lets us easily track an array of assets and indexes into that array for
/// contract transformation purposes
pub struct AssetArray {
    assets:     Vec<Asset>,
    assets_idx: HashMap<Address, usize>
}

impl Default for AssetArray {
    fn default() -> Self {
        Self::new()
    }
}

impl AssetArray {
    pub fn new() -> Self {
        Self { assets: Vec::new(), assets_idx: HashMap::new() }
    }

    pub fn get_asset_addr(&self, idx: usize) -> Address {
        self.assets
            .get(idx)
            .map(|asset| asset.addr)
            .unwrap_or_default()
    }

    pub fn add_or_get_asset_idx(&mut self, asset: Address) -> usize {
        *self.assets_idx.entry(asset).or_insert_with(|| {
            self.assets
                .push(Asset { addr: asset, borrow: 0, save: 0, settle: 0 });
            self.assets.len() - 1
        })
    }

    pub fn get_asset_array(&self) -> Vec<Asset> {
        self.assets.clone()
    }
}

impl From<AssetArray> for Vec<Asset> {
    fn from(val: AssetArray) -> Self {
        val.assets
    }
}
