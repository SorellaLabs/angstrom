use alloy::sol;
use pade_macro::{PadeDecode, PadeEncode};

pub mod angstrom;
pub mod asset;
pub mod rewards;
pub mod tob;

pub const POOL_CONFIG_STORE_ENTRY_SIZE: usize = 32;

sol! {
    #[derive(Debug, PadeEncode, PadeDecode)]
    struct Asset {
        address addr;
        uint128 borrow;
        uint128 save;
        uint128 settle;
    }

    #[derive(Debug, PadeEncode, PadeDecode)]
    struct Pair {
        uint16 index0;
        uint16 index1;
        uint16 store_index;
        uint256 price_1over0;
    }
}
