use alloy_sol_types::{eip712_domain, sol, Eip712Domain};
use bytes::Bytes;
use ethers::types::H256;
use hex_literal::hex;
use reth_primitives::bytes::BytesMut;
use reth_rlp::{Encodable as REncodable, RlpDecodable, RlpEncodable};
use revm::primitives::{TransactTo, TxEnv};
use serde::{Deserialize, Serialize};

use super::{HookSim, Signature, ANGSTROM_CONTRACT_ADDR};

const DOMAIN: Eip712Domain = alloy_sol_types::eip712_domain!(
   name: "Angstrom",
   version: "1",
);

sol! {
    enum OrderType {
        User,
        Searcher,
        Limit,
        UserFallible,
        SearcherFallible
    }
    type Currency is address;
    /// @notice The struct that the user signs using EIP-721.
    /// Canonical Angstrom order struct.
    struct Order {
        /// @member The user provided nonce, can only be used once.
        uint256 nonce;
        /// @member The order's type, can enable partial fills or fallible hooks.
        OrderType orderType;
        /// @member The input currency for the order.
        Currency currencyIn;
        /// @member The output currency for the order.
        Currency currencyOut;
        /// @member The (maximum) amount of input currency.
        uint128 amountIn;
        /// @member The minimum amount of output currency.
        uint128 amountOutMin;
        /// @member The order cannot be executed after this timestamp.
        uint256 deadline;
        /// @member An optional user provided hook to run before collecting input
        ///         payment.
        bytes preHook;
        /// @member An optional user provided hook to run after paying the output.
        bytes postHook;
    }
}

pub struct RawOrder {
    pub order:     Order,
    pub signature: Signature
}

impl TryInto<HookSim> for RawOrder {
    type Error = anyhow::Error;

    fn try_into(self) -> Result<HookSim, Self::Error> {
        let mut msg = Vec::new();
        msg.extend(SEARCHER_TYPE_HASH.to_fixed_bytes());
        msg.extend(self.order.pool);
        msg.extend(self.order.token_in.to_fixed_bytes());
        msg.extend(self.order.token_out.to_fixed_bytes());
        msg.extend(self.order.amount_in.to_be_bytes());
        msg.extend(self.order.amount_out_min.to_be_bytes());

        let mut deadbuf = BytesMut::new();
        self.order.deadline.to_big_endian(&mut deadbuf);
        msg.extend(deadbuf.to_vec());
        let mut bribe = BytesMut::new();
        self.order.bribe.to_big_endian(&mut bribe);
        msg.extend(bribe.to_vec());
        msg.extend(keccak256(&self.order.pre_hook));
        msg.extend(keccak256(&self.order.post_hook));

        let digest = keccak256(msg);
        let addr = self.signature.recover(digest)?;

        Ok(HookSim {
            tx: super::SearcherOrUser::Searcher(self.clone()),
            pre_hook: self.order.pre_hook,
            amount_in_req: self.order.amount_in,
            amount_in_token: self.order.token_in,
            post_hook: self.order.post_hook,
            amount_out_lim: self.order.amount_out_min,
            amount_out_token: self.order.token_out,
            addr
        })
    }
}

impl From<RawOrder> for H256 {
    fn from(value: RawOrder) -> Self {
        let mut buf = BytesMut::new();
        REncodable::encode(&value, &mut buf);

        H256(ethers_core::utils::keccak256(buf))
    }
}

impl Into<TxEnv> for RawOrder {
    fn into(self) -> TxEnv {
        let mut data = BytesMut::new();
        REncodable::encode(&self, &mut data);

        TxEnv {
            caller:           Default::default(),
            gas_limit:        self.order.gas_cap.as_u64(),
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
