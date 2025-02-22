use std::hash::Hash;

use alloy::primitives::{Address, B256, U256};
use pade::PadeDecode;
use pade_macro::{PadeDecode, PadeEncode};
use serde::{Deserialize, Serialize};

use crate::{
    contract_payloads::{Asset, Pair, Signature},
    primitive::ANGSTROM_DOMAIN,
    sol_bindings::{
        grouped_orders::OrderWithStorageData,
        rpc_orders::{OmitOrderMeta, TopOfBlockOrder as RpcTopOfBlockOrder},
        RawPoolOrder
    }
};

// This currently exists in types::sol_bindings as well, but that one is
// outdated so I'm building a new one here for now and then migrating
#[derive(
    PadeEncode, PadeDecode, Clone, Default, Debug, Hash, PartialEq, Eq, Serialize, Deserialize,
)]
pub struct TopOfBlockOrder {
    pub use_internal:     bool,
    pub quantity_in:      u128,
    pub quantity_out:     u128,
    pub max_gas_asset_0:  u128,
    pub gas_used_asset_0: u128,
    pub pairs_index:      u16,
    pub zero_for_1:       bool,
    pub recipient:        Option<Address>,
    pub signature:        Signature
}

impl TopOfBlockOrder {
    // eip-712 hash_struct. is a pain since we need to reconstruct values.
    fn recover_order(&self, pair: &[Pair], asset: &[Asset], block: u64) -> RpcTopOfBlockOrder {
        let pair = &pair[self.pairs_index as usize];
        RpcTopOfBlockOrder {
            quantity_in:     self.quantity_in,
            recipient:       self.recipient.unwrap_or_default(),
            quantity_out:    self.quantity_out,
            asset_in:        if self.zero_for_1 {
                asset[pair.index0 as usize].addr
            } else {
                asset[pair.index1 as usize].addr
            },
            asset_out:       if !self.zero_for_1 {
                asset[pair.index0 as usize].addr
            } else {
                asset[pair.index1 as usize].addr
            },
            use_internal:    self.use_internal,
            max_gas_asset0:  self.max_gas_asset_0,
            valid_for_block: block,
            meta:            Default::default()
        }
    }

    pub fn order_hash(&self, pair: &[Pair], asset: &[Asset], block: u64) -> B256 {
        self.recover_order(pair, asset, block).order_hash()
    }

    pub fn signing_hash(&self, pair: &[Pair], asset: &[Asset], block: u64) -> B256 {
        let order = self.recover_order(pair, asset, block);
        order.no_meta_eip712_signing_hash(&ANGSTROM_DOMAIN)
    }

    pub fn of_max_gas(
        internal: &OrderWithStorageData<RpcTopOfBlockOrder>,
        pairs_index: u16
    ) -> Self {
        assert!(internal.is_valid_signature());
        let quantity_in = internal.quantity_in;
        let quantity_out = internal.quantity_out;
        let recipient = Some(internal.recipient);
        // Zero_for_1 is an Ask, an Ask is NOT a bid
        let zero_for_1 = !internal.is_bid;
        let sig_bytes = internal.meta.signature.to_vec();
        let decoded_signature =
            alloy::primitives::PrimitiveSignature::pade_decode(&mut sig_bytes.as_slice(), None)
                .unwrap();
        let signature = Signature::from(decoded_signature);
        Self {
            use_internal: internal.use_internal,
            quantity_in,
            quantity_out,
            max_gas_asset_0: internal.max_gas_asset0,
            // set as max so we can use the sim to verify values.
            gas_used_asset_0: internal.priority_data.gas.to(),
            pairs_index,
            zero_for_1,
            recipient,
            signature
        }
    }

    pub fn of(
        internal: &OrderWithStorageData<RpcTopOfBlockOrder>,
        shared_gas: U256,
        pairs_index: u16
    ) -> eyre::Result<Self> {
        let quantity_in = internal.quantity_in;
        let quantity_out = internal.quantity_out;
        let recipient = Some(internal.recipient);
        // Zero_for_1 is an Ask, an Ask is NOT a bid
        let zero_for_1 = !internal.is_bid;
        let sig_bytes = internal.meta.signature.to_vec();
        let decoded_signature =
            alloy::primitives::PrimitiveSignature::pade_decode(&mut sig_bytes.as_slice(), None)
                .unwrap();
        let signature = Signature::from(decoded_signature);
        let used_gas: u128 = (internal.priority_data.gas).saturating_to();

        if used_gas > internal.max_gas_asset0 {
            return Err(eyre::eyre!("order went over gas limit"))
        }

        Ok(Self {
            use_internal: internal.use_internal,
            quantity_in,
            quantity_out,
            max_gas_asset_0: internal.max_gas_asset0,
            // set as max so we can use the sim to verify values.
            gas_used_asset_0: used_gas,
            pairs_index,
            zero_for_1,
            recipient,
            signature
        })
    }
}
