//! extension functionality to sol types

pub mod contract_bundle_ext;
pub mod grouped_orders;
pub mod top_of_block_ext;

pub trait FetchAssetIndexes {
    fn get_token_in(&self) -> u16;
    fn get_token_out(&self) -> u16;
}
