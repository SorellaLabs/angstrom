use bytes::Bytes;
use ethers_core::{
    types::{Address, H160, I256, U256},
    utils::keccak256
};
use hex_literal::hex;
use reth_primitives::bytes;
use reth_rlp::{Decodable, DecodeError, Encodable, RlpDecodable, RlpEncodable};
use serde::{Deserialize, Serialize};

mod bundle;
mod order_types;
mod signature;
mod submission;
mod users;

pub use bundle::*;
pub use order_types::*;
pub use signature::*;
pub use submission::*;
pub use users::*;

/// 1234567890abcedf1234567890abcdef12345678
pub const ANGSTROM_CONTRACT_ADDR: H160 = H160(hex!("1234567890abcedf1234567890abcdef12345678"));

/// the call address, the bytes to be called.
pub type HookCall = (Address, Bytes);

impl From<PoolKey> for [u8; 32] {
    fn from(value: PoolKey) -> Self {
        keccak256(ethers::abi::AbiEncode::encode(value))
    }
}

impl Encodable for CurrencySettlement {
    fn encode(&self, out: &mut dyn bytes::BufMut) {
        self.currency.encode(out);
        self.amount_net.into_raw().encode(out);
    }

    fn length(&self) -> usize {
        self.amount_net.into_raw().length()
    }
}

impl Decodable for CurrencySettlement {
    fn decode(buf: &mut &[u8]) -> Result<Self, DecodeError> {
        let currency = Address::decode(buf)?;
        let amount_net = U256::decode(buf)?;
        Ok(Self { currency, amount_net: I256::from_raw(amount_net) })
    }
}

#[derive(Debug)]
pub struct ExtCallSim {
    pub tx:               Order,
    // the address of the user.
    pub addr:             Address,
    // gas in
    pub pre_hook:         Bytes,
    pub amount_in_req:    u128,
    pub amount_in_token:  Address,
    // gas out
    pub post_hook:        Bytes,
    pub amount_out_lim:   u128,
    pub amount_out_token: Address
}

impl ExtCallSim {
    pub fn pre_hook(&self) -> HookCall {
        let addr = Address::from_slice(&self.pre_hook[0..19]);

        (addr, Bytes::copy_from_slice(&self.pre_hook[20..]))
    }

    pub fn post_hook(&self) -> HookCall {
        let addr = Address::from_slice(&self.post_hook[0..19]);

        (addr, Bytes::copy_from_slice(&self.post_hook[20..]))
    }
}
