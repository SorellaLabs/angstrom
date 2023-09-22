use std::collections::HashMap;

use ethers_core::types::{H256, U256};
use reth_primitives::bytes::BytesMut;
use reth_rlp::Encodable;
use revm::primitives::{TransactTo, TxEnv, B160, U256 as RU256};
use serde::{Deserialize, Serialize};

use super::{
    CurrencySettlement, PoolFees, PoolSwap, RawLvrSettlement, RawUserSettlement, RlpDecodable,
    RlpEncodable, SafeTx, SimmedLvrSettlement, SimmedUserSettlement, ANGSTROM_CONTRACT_ADDR
};

#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
    RlpDecodable,
    RlpEncodable,
    PartialEq,
    Eq,
    Hash,
    ethers_contract::EthAbiType,
    ethers_contract::EthAbiCodec,
)]
pub struct SimmedBundle {
    // Univ4 swaps
    pub raw:      RawBundle,
    pub gas_used: U256
}

impl SimmedBundle {
    pub fn get_cumulative_lp_bribe(&self) -> u128 {
        self.raw.get_cumulative_lp_bribe()
    }
}

impl From<SimmedBundle> for H256 {
    fn from(value: SimmedBundle) -> Self {
        let mut buf = BytesMut::new();
        value.encode(&mut buf);

        H256(ethers_core::utils::keccak256(buf))
    }
}

impl SimmedBundle {
    pub fn into_call_data(&self) -> SafeTx {
        todo!()
    }
}

#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
    RlpDecodable,
    RlpEncodable,
    PartialEq,
    Eq,
    Hash,
    ethers_contract::EthAbiType,
    ethers_contract::EthAbiCodec,
)]
pub struct RawBundle {
    pub lvr:        Vec<SimmedLvrSettlement>,
    pub users:      Vec<SimmedUserSettlement>,
    pub swaps:      Vec<PoolSwap>,
    pub currencies: Vec<CurrencySettlement>,
    pub pools:      Vec<PoolFees>
}

impl From<RawBundle> for H256 {
    fn from(value: RawBundle) -> Self {
        let mut buf = BytesMut::new();
        value.encode(&mut buf);

        H256(ethers_core::utils::keccak256(buf))
    }
}

impl From<SimmedBundle> for RawBundle {
    fn from(value: SimmedBundle) -> Self {
        value.raw
    }
}

impl Into<TxEnv> for RawBundle {
    fn into(self) -> TxEnv {
        let mut gas_limit = U256::from_dec_str("0").unwrap();
        let _ = self.lvr.iter().map(|v| gas_limit += v.raw.order.gas_cap);
        let _ = self.users.iter().map(|v| gas_limit += v.raw.order.gas_cap);

        let mut data = BytesMut::new();
        self.encode(&mut data);

        TxEnv {
            caller:           Default::default(),
            gas_limit:        gas_limit.as_u64(),
            gas_price:        reth_primitives::U256::ZERO,
            gas_priority_fee: None,
            transact_to:      TransactTo::Call(ANGSTROM_CONTRACT_ADDR.into()),
            value:            reth_primitives::U256::ZERO,
            data:             data.to_vec().into(),
            chain_id:         Some(1),
            nonce:            None,
            access_list:      Default::default()
        }
    }
}

impl RawBundle {
    pub fn get_cumulative_lp_bribe(&self) -> u128 {
        todo!()
    }
}

#[derive(Debug, Clone)]
pub struct CallerInfo {
    pub address:   B160,
    pub nonce:     u64,
    pub overrides: HashMap<B160, HashMap<RU256, RU256>>
}
