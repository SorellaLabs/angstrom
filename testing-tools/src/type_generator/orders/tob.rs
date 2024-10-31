use alloy::{
    primitives::Address,
    signers::{local::LocalSigner, SignerSync}
};
use angstrom_types::sol_bindings::rpc_orders::{OmitOrderMeta, OrderMeta, TopOfBlockOrder};
use pade::PadeEncode;

use super::SigningInfo;

#[derive(Default, Debug)]
pub struct ToBOrderBuilder {
    recipient:    Option<Address>,
    asset_in:     Option<Address>,
    asset_out:    Option<Address>,
    quantity_in:  Option<u128>,
    quantity_out: Option<u128>,
    valid_block:  Option<u64>,
    signing_key:  Option<SigningInfo>
}

impl ToBOrderBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn recipient(self, recipient: Address) -> Self {
        Self { recipient: Some(recipient), ..self }
    }

    pub fn asset_in(self, asset_in: Address) -> Self {
        Self { asset_in: Some(asset_in), ..self }
    }

    pub fn asset_out(self, asset_out: Address) -> Self {
        Self { asset_out: Some(asset_out), ..self }
    }

    pub fn quantity_in(self, quantity_in: u128) -> Self {
        Self { quantity_in: Some(quantity_in), ..self }
    }

    pub fn quantity_out(self, quantity_out: u128) -> Self {
        Self { quantity_out: Some(quantity_out), ..self }
    }

    pub fn valid_block(self, valid_block: u64) -> Self {
        Self { valid_block: Some(valid_block), ..self }
    }

    pub fn signing_key(self, signing_key: Option<SigningInfo>) -> Self {
        Self { signing_key, ..self }
    }

    pub fn build(self) -> TopOfBlockOrder {
        let mut order = TopOfBlockOrder {
            asset_in: self.asset_in.unwrap_or_default(),
            asset_out: self.asset_out.unwrap_or_default(),
            quantity_in: self.quantity_in.unwrap_or_default(),
            quantity_out: self.quantity_out.unwrap_or_default(),
            valid_for_block: self.valid_block.unwrap_or_default(),
            recipient: self.recipient.unwrap_or_else(|| Address::random()),
            ..Default::default()
        };
        if let Some(SigningInfo { domain, address, key }) = self.signing_key {
            let signer = LocalSigner::from_signing_key(key);
            let hash = order.no_meta_eip712_signing_hash(&domain);
            println!("Typehash: {:?}", order.eip712_type_hash());
            println!("Hash debug {:?}", hash);
            println!("Alt hash debug: {:?}", order.eip712_hash_struct());
            let sig = signer.sign_hash_sync(&hash).unwrap();
            println!("Signature debug: {:?}", sig);
            order.meta = OrderMeta {
                isEcdsa:   true,
                from:      address,
                signature: sig.pade_encode().into()
            };
        }
        order
    }
}
