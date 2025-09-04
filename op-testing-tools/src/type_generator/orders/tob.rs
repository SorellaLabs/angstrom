use alloy::{
    primitives::Address,
    signers::{SignerSync, local::PrivateKeySigner}
};
use angstrom_types::{
    primitive::{ANGSTROM_DOMAIN, AngstromSigner},
    sol_bindings::rpc_orders::{OmitOrderMeta, OrderMeta, TopOfBlockOrder}
};
use pade::PadeEncode;

#[derive(Default, Debug)]
pub struct ToBOrderBuilder {
    recipient:    Option<Address>,
    asset_in:     Option<Address>,
    asset_out:    Option<Address>,
    quantity_in:  Option<u128>,
    quantity_out: Option<u128>,
    max_gas:      Option<u128>,
    valid_block:  Option<u64>,
    use_internal: bool,
    signing_key:  Option<AngstromSigner<PrivateKeySigner>>
}

impl ToBOrderBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn max_gas(self, gas: u128) -> Self {
        if gas == 0 {
            return self;
        }
        Self { max_gas: Some(gas), ..self }
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

    pub fn signing_key(self, signing_key: Option<AngstromSigner<PrivateKeySigner>>) -> Self {
        Self { signing_key, ..self }
    }

    pub fn use_internal(self, use_internal: bool) -> Self {
        Self { use_internal, ..self }
    }

    pub fn build(self) -> TopOfBlockOrder {
        let mut order = TopOfBlockOrder {
            asset_in: self.asset_in.unwrap_or_default(),
            asset_out: self.asset_out.unwrap_or_default(),
            quantity_in: self.quantity_in.unwrap_or_default(),
            quantity_out: self.quantity_out.unwrap_or_default(),
            valid_for_block: self.valid_block.unwrap_or_default(),
            recipient: self.recipient.unwrap_or_default(),
            max_gas_asset0: self.max_gas.unwrap_or_else(|| {
                // zero for 1
                if self.asset_in < self.asset_out {
                    self.quantity_in.unwrap_or_default() / 2
                } else {
                    self.quantity_out.unwrap_or_default() / 2
                }
            }),
            use_internal: self.use_internal,
            ..Default::default()
        };
        if let Some(signer) = self.signing_key {
            let hash = order.no_meta_eip712_signing_hash(ANGSTROM_DOMAIN.get().unwrap());
            let sig = signer.sign_hash_sync(&hash).unwrap();
            order.meta = OrderMeta {
                isEcdsa:   true,
                from:      signer.address(),
                signature: sig.pade_encode().into()
            };
        }
        order
    }
}
