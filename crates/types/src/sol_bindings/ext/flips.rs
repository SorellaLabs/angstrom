use alloy::{primitives::U256, signers::SignerSync};
use pade::PadeEncode;

use super::{
    grouped_orders::{FlashVariants, GroupedVanillaOrder, StandingVariants},
    GenerateFlippedOrder
};
use crate::{
    matching::Ray,
    primitive::{AngstromSigner, ANGSTROM_DOMAIN},
    sol_bindings::{
        rpc_orders::{
            ExactFlashOrder, ExactStandingOrder, OmitOrderMeta, OrderMeta, PartialFlashOrder,
            PartialStandingOrder
        },
        RawPoolOrder
    }
};

impl GenerateFlippedOrder for ExactStandingOrder {
    fn flip(&self) -> Self
    where
        Self: Sized
    {
        let new_signer = AngstromSigner::random();

        let mut this = Self {
            asset_in: self.asset_out,
            asset_out: self.asset_in,
            amount: self.amount,
            max_extra_fee_asset0: self.max_extra_fee_asset0,
            exact_in: !self.exact_in,
            ..self.clone()
        };

        // sign new meta
        let hash = this.no_meta_eip712_signing_hash(&ANGSTROM_DOMAIN);
        let sig = new_signer.sign_hash_sync(&hash).unwrap();
        let addr = new_signer.address();
        this.meta =
            OrderMeta { isEcdsa: true, from: addr, signature: sig.pade_encode().into() };
        this
    }
}

impl GenerateFlippedOrder for PartialFlashOrder {
    fn flip(&self) -> Self
    where
        Self: Sized
    {
        let new_signer = AngstromSigner::random();
        let price = Ray::from(self.min_price);
        let amount_out_max = price.mul_quantity(U256::from(self.amount_in()));
        let min = price.mul_quantity(U256::from(self.min_amount_in));

        let mut this = Self {
            asset_in: self.asset_out,
            asset_out: self.asset_in,
            min_amount_in: min.to(),
            max_amount_in: amount_out_max.to(),
            max_extra_fee_asset0: amount_out_max.to(),
            ..self.clone()
        };

        // sign new meta
        let hash = this.no_meta_eip712_signing_hash(&ANGSTROM_DOMAIN);
        let sig = new_signer.sign_hash_sync(&hash).unwrap();
        let addr = new_signer.address();
        this.meta =
            OrderMeta { isEcdsa: true, from: addr, signature: sig.pade_encode().into() };
        this
    }
}

impl GenerateFlippedOrder for ExactFlashOrder {
    fn flip(&self) -> Self
    where
        Self: Sized
    {
        let new_signer = AngstromSigner::random();

        let mut this = Self {
            asset_in: self.asset_out,
            asset_out: self.asset_in,
            amount: self.amount,
            max_extra_fee_asset0: self.max_extra_fee_asset0,
            exact_in: !self.exact_in,
            ..self.clone()
        };

        // sign new meta
        let hash = this.no_meta_eip712_signing_hash(&ANGSTROM_DOMAIN);
        let sig = new_signer.sign_hash_sync(&hash).unwrap();
        let addr = new_signer.address();
        this.meta =
            OrderMeta { isEcdsa: true, from: addr, signature: sig.pade_encode().into() };
        this
    }
}

impl GenerateFlippedOrder for PartialStandingOrder {
    fn flip(&self) -> Self
    where
        Self: Sized
    {
        let new_signer = AngstromSigner::random();
        let price = Ray::from(self.min_price);
        let amount_out_max = price.mul_quantity(U256::from(self.amount_in()));
        let min = price.mul_quantity(U256::from(self.min_amount_in));

        let mut this = Self {
            asset_in: self.asset_out,
            asset_out: self.asset_in,
            min_amount_in: min.to(),
            max_amount_in: amount_out_max.to(),
            max_extra_fee_asset0: amount_out_max.to(),
            ..self.clone()
        };

        // sign new meta
        let hash = this.no_meta_eip712_signing_hash(&ANGSTROM_DOMAIN);
        let sig = new_signer.sign_hash_sync(&hash).unwrap();
        let addr = new_signer.address();
        this.meta =
            OrderMeta { isEcdsa: true, from: addr, signature: sig.pade_encode().into() };
        this
    }
}

impl GenerateFlippedOrder for GroupedVanillaOrder {
    fn flip(&self) -> Self
    where
        Self: Sized
    {
        match self {
            GroupedVanillaOrder::Standing(s) => GroupedVanillaOrder::Standing(s.flip()),
            GroupedVanillaOrder::KillOrFill(s) => GroupedVanillaOrder::KillOrFill(s.flip())
        }
    }
}

impl GenerateFlippedOrder for StandingVariants {
    fn flip(&self) -> Self
    where
        Self: Sized
    {
        match self {
            StandingVariants::Partial(s) => StandingVariants::Partial(s.flip()),
            StandingVariants::Exact(s) => StandingVariants::Exact(s.flip())
        }
    }
}

impl GenerateFlippedOrder for FlashVariants {
    fn flip(&self) -> Self
    where
        Self: Sized
    {
        match self {
            FlashVariants::Partial(s) => FlashVariants::Partial(s.flip()),
            FlashVariants::Exact(s) => FlashVariants::Exact(s.flip())
        }
    }
}
