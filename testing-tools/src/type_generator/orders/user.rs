use alloy::{
    primitives::{Address, U256},
    signers::SignerSync
};
use alloy_primitives::aliases::U40;
use angstrom_types::{
    matching::Ray,
    primitive::{AngstromSigner, ANGSTROM_DOMAIN},
    sol_bindings::{
        grouped_orders::{FlashVariants, GroupedVanillaOrder, StandingVariants},
        rpc_orders::{
            ExactFlashOrder, ExactStandingOrder, OmitOrderMeta, OrderMeta, PartialFlashOrder,
            PartialStandingOrder
        }
    }
};
use pade::PadeEncode;

use super::StoredOrderBuilder;

#[derive(Clone, Debug, Default)]
pub struct UserOrderBuilder {
    /// If the order is not a Standing order, it is KillOrFill
    is_standing: bool,
    /// If the order is not an Exact order, it is Partial
    is_exact:    bool,
    exact_in:    bool,
    block:       u64,
    nonce:       u64,
    recipient:   Address,
    asset_in:    Address,
    asset_out:   Address,
    amount:      u128,
    min_price:   Ray,
    deadline:    U256,
    signing_key: Option<AngstromSigner>
}

impl UserOrderBuilder {
    pub fn new() -> Self {
        Self { ..Default::default() }
    }

    pub fn standing(self) -> Self {
        Self { is_standing: true, ..self }
    }

    pub fn kill_or_fill(self) -> Self {
        Self { is_standing: false, ..self }
    }

    /// Sets the order to be kill-or-fill or standing by explicit boolean
    pub fn is_standing(self, is_standing: bool) -> Self {
        Self { is_standing, ..self }
    }

    pub fn exact(self) -> Self {
        Self { is_exact: true, ..self }
    }

    pub fn partial(self) -> Self {
        Self { is_exact: false, ..self }
    }

    /// Sets the order to be exact or partial by explicit boolean
    pub fn is_exact(self, is_exact: bool) -> Self {
        Self { is_exact, ..self }
    }

    pub fn block(self, block: u64) -> Self {
        Self { block, ..self }
    }

    pub fn deadline(self, deadline: U256) -> Self {
        Self { deadline, ..self }
    }

    pub fn nonce(self, nonce: u64) -> Self {
        Self { nonce, ..self }
    }

    pub fn recipient(self, recipient: Address) -> Self {
        Self { recipient, ..self }
    }

    pub fn asset_in(self, asset_in: Address) -> Self {
        Self { asset_in, ..self }
    }

    pub fn asset_out(self, asset_out: Address) -> Self {
        Self { asset_out, ..self }
    }

    pub fn amount(self, amount: u128) -> Self {
        Self { amount, ..self }
    }

    pub fn exact_in(self, exact_in: bool) -> Self {
        Self { exact_in, ..self }
    }

    pub fn min_price(self, min_price: Ray) -> Self {
        Self { min_price, ..self }
    }

    pub fn bid_min_price(self, min_price: Ray) -> Self {
        Self { min_price: min_price.inv_ray_round(true), ..self }
    }

    pub fn signing_key(self, signing_key: Option<AngstromSigner>) -> Self {
        Self { signing_key, ..self }
    }

    pub fn build(self) -> GroupedVanillaOrder {
        match (self.is_standing, self.is_exact) {
            (true, true) => {
                let mut order = ExactStandingOrder {
                    asset_in: self.asset_in,
                    asset_out: self.asset_out,
                    amount: self.amount,
                    max_extra_fee_asset0: self.amount,
                    min_price: *self.min_price,
                    recipient: self.recipient,
                    nonce: self.nonce,
                    exact_in: self.exact_in,
                    deadline: U40::from(self.deadline.to::<u32>()),
                    ..Default::default()
                };
                if let Some(signer) = self.signing_key {
                    let hash = order.no_meta_eip712_signing_hash(&ANGSTROM_DOMAIN);
                    let sig = signer.sign_hash_sync(&hash).unwrap();
                    order.meta = OrderMeta {
                        isEcdsa:   true,
                        from:      signer.address(),
                        signature: sig.pade_encode().into()
                    };
                }
                GroupedVanillaOrder::Standing(StandingVariants::Exact(order))
            }
            (true, false) => {
                let mut order = PartialStandingOrder {
                    asset_in: self.asset_in,
                    asset_out: self.asset_out,
                    max_amount_in: self.amount,
                    max_extra_fee_asset0: self.amount,
                    nonce: self.nonce,
                    min_price: *self.min_price,
                    recipient: self.recipient,
                    deadline: U40::from(self.deadline.to::<u32>()),
                    ..Default::default()
                };
                if let Some(signer) = self.signing_key {
                    let hash = order.no_meta_eip712_signing_hash(&ANGSTROM_DOMAIN);
                    let sig = signer.sign_hash_sync(&hash).unwrap();
                    order.meta = OrderMeta {
                        isEcdsa:   true,
                        from:      signer.address(),
                        signature: sig.pade_encode().into()
                    };
                }
                GroupedVanillaOrder::Standing(StandingVariants::Partial(order))
            }
            (false, true) => {
                let mut order = ExactFlashOrder {
                    valid_for_block: self.block,
                    asset_in: self.asset_in,
                    asset_out: self.asset_out,
                    max_extra_fee_asset0: self.amount,
                    amount: self.amount,
                    min_price: *self.min_price,
                    recipient: self.recipient,
                    exact_in: self.exact_in,
                    ..Default::default()
                };
                if let Some(signer) = self.signing_key {
                    let hash = order.no_meta_eip712_signing_hash(&ANGSTROM_DOMAIN);
                    let sig = signer.sign_hash_sync(&hash).unwrap();
                    order.meta = OrderMeta {
                        isEcdsa:   true,
                        from:      signer.address(),
                        signature: sig.pade_encode().into()
                    };
                }
                GroupedVanillaOrder::KillOrFill(FlashVariants::Exact(order))
            }
            (false, false) => {
                let mut order = PartialFlashOrder {
                    valid_for_block: self.block,
                    asset_in: self.asset_in,
                    asset_out: self.asset_out,
                    max_extra_fee_asset0: self.amount,
                    max_amount_in: self.amount,
                    min_price: *self.min_price,
                    recipient: self.recipient,
                    ..Default::default()
                };
                if let Some(signer) = self.signing_key {
                    let hash = order.no_meta_eip712_signing_hash(&ANGSTROM_DOMAIN);
                    let sig = signer.sign_hash_sync(&hash).unwrap();
                    order.meta = OrderMeta {
                        isEcdsa:   true,
                        from:      signer.address(),
                        signature: sig.pade_encode().into()
                    };
                }
                GroupedVanillaOrder::KillOrFill(FlashVariants::Partial(order))
            }
        }
    }

    /// Lets us chain right into the storage wrapper
    pub fn with_storage(self) -> StoredOrderBuilder {
        let block = self.block;
        StoredOrderBuilder::new(self.build()).valid_block(block)
    }
}
