use alloy::{
    primitives::{Address, Bytes, FixedBytes},
    sol
};
use pade_macro::{PadeDecode, PadeEncode};
use serde::{Deserialize, Serialize};

pub mod angstrom;
pub mod asset;
pub mod rewards;
pub mod tob;

pub const CONFIG_STORE_SLOT: u32 = 2;
pub const POOL_CONFIG_STORE_ENTRY_SIZE: usize = 32;

sol! {
    #[derive(Debug, Default, PadeEncode, PadeDecode)]
    struct Asset {
        address addr;
        uint128 borrow;
        uint128 save;
        uint128 settle;
    }

    #[derive(Debug,Default, PadeEncode, PadeDecode)]
    struct Pair {
        uint16 index0;
        uint16 index1;
        uint16 store_index;
        uint256 price_1over0;
    }
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, PadeEncode, PadeDecode, Serialize, Deserialize)]
pub enum Signature {
    Contract { from: Address, signature: Bytes },
    Ecdsa { v: u8, r: FixedBytes<32>, s: FixedBytes<32> }
}

impl Default for Signature {
    fn default() -> Self {
        Self::Contract { from: Address::default(), signature: Bytes::default() }
    }
}

impl From<alloy::primitives::Signature> for Signature {
    fn from(value: alloy::primitives::Signature) -> Self {
        let v = value.v().y_parity_byte();
        // TODO:  Make this robust, right now it Just Works
        let r: FixedBytes<32> = value.r().into();
        let s: FixedBytes<32> = value.s().into();
        Self::Ecdsa { v, r, s }
    }
}
