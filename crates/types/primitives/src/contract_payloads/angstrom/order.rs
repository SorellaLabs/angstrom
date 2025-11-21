use alloy_primitives::{Address, B256, Bytes, U256, aliases::U40};
use pade_macro::{PadeDecode, PadeEncode};
use serde::{Deserialize, Serialize};

use crate::{
    contract_payloads::{Asset, Pair, Signature},
    primitive::ANGSTROM_DOMAIN,
    sol_bindings::{
        RawPoolOrder,
        grouped_orders::AllOrders,
        rpc_orders::{
            ExactFlashOrder, ExactStandingOrder, OmitOrderMeta, OrderMeta, PartialFlashOrder,
            PartialStandingOrder
        }
    }
};

#[derive(
    Debug, Clone, PadeEncode, PadeDecode, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize,
)]
pub enum OrderQuantities {
    Exact { quantity: u128 },
    Partial { min_quantity_in: u128, max_quantity_in: u128, filled_quantity: u128 }
}

impl OrderQuantities {
    pub fn fetch_max_amount(&self) -> u128 {
        match self {
            Self::Exact { quantity } => *quantity,
            Self::Partial { max_quantity_in, .. } => *max_quantity_in
        }
    }
}

#[derive(
    Debug, Clone, PadeEncode, PadeDecode, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize,
)]
pub struct StandingValidation {
    nonce:    u64,
    // 40 bits wide in reality
    #[pade_width(5)]
    deadline: u64
}

impl StandingValidation {
    pub fn new(nonce: u64, deadline: u64) -> Self {
        Self { nonce, deadline }
    }

    pub fn nonce(&self) -> u64 {
        self.nonce
    }

    pub fn deadline(&self) -> u64 {
        self.deadline
    }
}

#[derive(
    Debug, Clone, PadeEncode, PadeDecode, PartialEq, Eq, Ord, PartialOrd, Serialize, Deserialize,
)]
pub struct UserOrder {
    pub ref_id:               u32,
    pub use_internal:         bool,
    pub pair_index:           u16,
    pub min_price:            U256,
    pub recipient:            Option<Address>,
    pub hook_data:            Option<Bytes>,
    pub zero_for_one:         bool,
    pub standing_validation:  Option<StandingValidation>,
    pub order_quantities:     OrderQuantities,
    pub max_extra_fee_asset0: u128,
    pub extra_fee_asset0:     u128,
    pub exact_in:             bool,
    pub signature:            Signature
}

impl UserOrder {
    pub fn recover_signer(&self, pair: &[Pair], asset: &[Asset], block: u64) -> Address {
        self.signature
            .recover_signer(self.signing_hash(pair, asset, block))
    }

    pub fn recover_order(&self, pair: &[Pair], asset: &[Asset], block: u64) -> AllOrders {
        let from = self.recover_signer(pair, asset, block);
        let pair = &pair[self.pair_index as usize];

        match self.order_quantities {
            OrderQuantities::Exact { quantity } => {
                if let Some(validation) = &self.standing_validation {
                    // exact standing
                    AllOrders::ExactStanding(ExactStandingOrder {
                        ref_id:               self.ref_id,
                        exact_in:             self.exact_in,
                        use_internal:         self.use_internal,
                        asset_in:             if self.zero_for_one {
                            asset[pair.index0 as usize].addr
                        } else {
                            asset[pair.index1 as usize].addr
                        },
                        asset_out:            if !self.zero_for_one {
                            asset[pair.index0 as usize].addr
                        } else {
                            asset[pair.index1 as usize].addr
                        },
                        recipient:            self.recipient.unwrap_or_default(),
                        nonce:                validation.nonce,
                        deadline:             U40::from_limbs([validation.deadline]),
                        amount:               quantity,
                        min_price:            self.min_price,
                        hook_data:            self.hook_data.clone().unwrap_or_default(),
                        max_extra_fee_asset0: self.max_extra_fee_asset0,
                        meta:                 OrderMeta { from, ..Default::default() }
                    })
                } else {
                    // exact flash
                    AllOrders::ExactFlash(ExactFlashOrder {
                        ref_id:               self.ref_id,
                        exact_in:             self.exact_in,
                        use_internal:         self.use_internal,
                        asset_in:             if self.zero_for_one {
                            asset[pair.index0 as usize].addr
                        } else {
                            asset[pair.index1 as usize].addr
                        },
                        asset_out:            if !self.zero_for_one {
                            asset[pair.index0 as usize].addr
                        } else {
                            asset[pair.index1 as usize].addr
                        },
                        recipient:            self.recipient.unwrap_or_default(),
                        valid_for_block:      block,
                        amount:               quantity,
                        min_price:            self.min_price,
                        hook_data:            self.hook_data.clone().unwrap_or_default(),
                        max_extra_fee_asset0: self.max_extra_fee_asset0,
                        meta:                 OrderMeta { from, ..Default::default() }
                    })
                }
            }
            OrderQuantities::Partial { min_quantity_in, max_quantity_in, .. } => {
                if let Some(validation) = &self.standing_validation {
                    AllOrders::PartialStanding(PartialStandingOrder {
                        ref_id:               self.ref_id,
                        use_internal:         self.use_internal,
                        asset_in:             if self.zero_for_one {
                            asset[pair.index0 as usize].addr
                        } else {
                            asset[pair.index1 as usize].addr
                        },
                        asset_out:            if !self.zero_for_one {
                            asset[pair.index0 as usize].addr
                        } else {
                            asset[pair.index1 as usize].addr
                        },
                        recipient:            self.recipient.unwrap_or_default(),
                        deadline:             U40::from_limbs([validation.deadline]),
                        nonce:                validation.nonce,
                        min_amount_in:        min_quantity_in,
                        max_amount_in:        max_quantity_in,
                        min_price:            self.min_price,
                        hook_data:            self.hook_data.clone().unwrap_or_default(),
                        max_extra_fee_asset0: self.max_extra_fee_asset0,
                        meta:                 OrderMeta { from, ..Default::default() }
                    })
                } else {
                    AllOrders::PartialFlash(PartialFlashOrder {
                        ref_id:               self.ref_id,
                        use_internal:         self.use_internal,
                        asset_in:             if self.zero_for_one {
                            asset[pair.index0 as usize].addr
                        } else {
                            asset[pair.index1 as usize].addr
                        },
                        asset_out:            if !self.zero_for_one {
                            asset[pair.index0 as usize].addr
                        } else {
                            asset[pair.index1 as usize].addr
                        },
                        recipient:            self.recipient.unwrap_or_default(),
                        valid_for_block:      block,
                        max_amount_in:        max_quantity_in,
                        min_amount_in:        min_quantity_in,
                        min_price:            self.min_price,
                        hook_data:            self.hook_data.clone().unwrap_or_default(),
                        max_extra_fee_asset0: self.max_extra_fee_asset0,
                        meta:                 OrderMeta { from, ..Default::default() }
                    })
                }
            }
        }
    }

    pub fn order_hash(&self, pair: &[Pair], asset: &[Asset], block: u64) -> B256 {
        // need so we can generate proper order hash.
        let from = self.recover_signer(pair, asset, block);
        let pair = &pair[self.pair_index as usize];

        match self.order_quantities {
            OrderQuantities::Exact { quantity } => {
                if let Some(validation) = &self.standing_validation {
                    // exact standing
                    ExactStandingOrder {
                        ref_id:               self.ref_id,
                        exact_in:             self.exact_in,
                        use_internal:         self.use_internal,
                        asset_in:             if self.zero_for_one {
                            asset[pair.index0 as usize].addr
                        } else {
                            asset[pair.index1 as usize].addr
                        },
                        asset_out:            if !self.zero_for_one {
                            asset[pair.index0 as usize].addr
                        } else {
                            asset[pair.index1 as usize].addr
                        },
                        recipient:            self.recipient.unwrap_or_default(),
                        nonce:                validation.nonce,
                        deadline:             U40::from_limbs([validation.deadline]),
                        amount:               quantity,
                        min_price:            self.min_price,
                        hook_data:            self.hook_data.clone().unwrap_or_default(),
                        max_extra_fee_asset0: self.max_extra_fee_asset0,
                        meta:                 OrderMeta { from, ..Default::default() }
                    }
                    .order_hash()
                } else {
                    // exact flash
                    ExactFlashOrder {
                        ref_id:               self.ref_id,
                        exact_in:             self.exact_in,
                        use_internal:         self.use_internal,
                        asset_in:             if self.zero_for_one {
                            asset[pair.index0 as usize].addr
                        } else {
                            asset[pair.index1 as usize].addr
                        },
                        asset_out:            if !self.zero_for_one {
                            asset[pair.index0 as usize].addr
                        } else {
                            asset[pair.index1 as usize].addr
                        },
                        recipient:            self.recipient.unwrap_or_default(),
                        valid_for_block:      block,
                        amount:               quantity,
                        min_price:            self.min_price,
                        hook_data:            self.hook_data.clone().unwrap_or_default(),
                        max_extra_fee_asset0: self.max_extra_fee_asset0,
                        meta:                 OrderMeta { from, ..Default::default() }
                    }
                    .order_hash()
                }
            }
            OrderQuantities::Partial { min_quantity_in, max_quantity_in, .. } => {
                if let Some(validation) = &self.standing_validation {
                    PartialStandingOrder {
                        ref_id:               self.ref_id,
                        use_internal:         self.use_internal,
                        asset_in:             if self.zero_for_one {
                            asset[pair.index0 as usize].addr
                        } else {
                            asset[pair.index1 as usize].addr
                        },
                        asset_out:            if !self.zero_for_one {
                            asset[pair.index0 as usize].addr
                        } else {
                            asset[pair.index1 as usize].addr
                        },
                        recipient:            self.recipient.unwrap_or_default(),
                        deadline:             U40::from_limbs([validation.deadline]),
                        nonce:                validation.nonce,
                        min_amount_in:        min_quantity_in,
                        max_amount_in:        max_quantity_in,
                        min_price:            self.min_price,
                        hook_data:            self.hook_data.clone().unwrap_or_default(),
                        max_extra_fee_asset0: self.max_extra_fee_asset0,
                        meta:                 OrderMeta { from, ..Default::default() }
                    }
                    .order_hash()
                } else {
                    PartialFlashOrder {
                        ref_id:               self.ref_id,
                        use_internal:         self.use_internal,
                        asset_in:             if self.zero_for_one {
                            asset[pair.index0 as usize].addr
                        } else {
                            asset[pair.index1 as usize].addr
                        },
                        asset_out:            if !self.zero_for_one {
                            asset[pair.index0 as usize].addr
                        } else {
                            asset[pair.index1 as usize].addr
                        },
                        recipient:            self.recipient.unwrap_or_default(),
                        valid_for_block:      block,
                        max_amount_in:        max_quantity_in,
                        min_amount_in:        min_quantity_in,
                        min_price:            self.min_price,
                        hook_data:            self.hook_data.clone().unwrap_or_default(),
                        max_extra_fee_asset0: self.max_extra_fee_asset0,
                        meta:                 OrderMeta { from, ..Default::default() }
                    }
                    .order_hash()
                }
            }
        }
    }

    pub fn signing_hash(&self, pair: &[Pair], asset: &[Asset], block: u64) -> B256 {
        let pair = &pair[self.pair_index as usize];
        match self.order_quantities {
            OrderQuantities::Exact { quantity } => {
                if let Some(validation) = &self.standing_validation {
                    // exact standing
                    let recovered = ExactStandingOrder {
                        ref_id: self.ref_id,
                        exact_in: self.exact_in,
                        use_internal: self.use_internal,
                        asset_in: if self.zero_for_one {
                            asset[pair.index0 as usize].addr
                        } else {
                            asset[pair.index1 as usize].addr
                        },
                        asset_out: if !self.zero_for_one {
                            asset[pair.index0 as usize].addr
                        } else {
                            asset[pair.index1 as usize].addr
                        },
                        recipient: self.recipient.unwrap_or_default(),
                        nonce: validation.nonce,
                        deadline: U40::from_limbs([validation.deadline]),
                        amount: quantity,
                        min_price: self.min_price,
                        hook_data: self.hook_data.clone().unwrap_or_default(),
                        max_extra_fee_asset0: self.max_extra_fee_asset0,
                        ..Default::default()
                    };
                    recovered.no_meta_eip712_signing_hash(ANGSTROM_DOMAIN.get().unwrap())
                } else {
                    // exact flash
                    let recovered = ExactFlashOrder {
                        ref_id: self.ref_id,
                        exact_in: self.exact_in,
                        use_internal: self.use_internal,
                        asset_in: if self.zero_for_one {
                            asset[pair.index0 as usize].addr
                        } else {
                            asset[pair.index1 as usize].addr
                        },
                        asset_out: if !self.zero_for_one {
                            asset[pair.index0 as usize].addr
                        } else {
                            asset[pair.index1 as usize].addr
                        },
                        recipient: self.recipient.unwrap_or_default(),
                        valid_for_block: block,
                        amount: quantity,
                        min_price: self.min_price,
                        hook_data: self.hook_data.clone().unwrap_or_default(),
                        max_extra_fee_asset0: self.max_extra_fee_asset0,
                        ..Default::default()
                    };

                    recovered.no_meta_eip712_signing_hash(ANGSTROM_DOMAIN.get().unwrap())
                }
            }
            OrderQuantities::Partial { min_quantity_in, max_quantity_in, .. } => {
                if let Some(validation) = &self.standing_validation {
                    let recovered = PartialStandingOrder {
                        ref_id: self.ref_id,
                        use_internal: self.use_internal,
                        asset_in: if self.zero_for_one {
                            asset[pair.index0 as usize].addr
                        } else {
                            asset[pair.index1 as usize].addr
                        },
                        asset_out: if !self.zero_for_one {
                            asset[pair.index0 as usize].addr
                        } else {
                            asset[pair.index1 as usize].addr
                        },
                        recipient: self.recipient.unwrap_or_default(),
                        deadline: U40::from_limbs([validation.deadline]),
                        nonce: validation.nonce,
                        min_amount_in: min_quantity_in,
                        max_amount_in: max_quantity_in,
                        min_price: self.min_price,
                        hook_data: self.hook_data.clone().unwrap_or_default(),
                        max_extra_fee_asset0: self.max_extra_fee_asset0,
                        ..Default::default()
                    };
                    recovered.no_meta_eip712_signing_hash(ANGSTROM_DOMAIN.get().unwrap())
                } else {
                    let recovered = PartialFlashOrder {
                        ref_id: self.ref_id,
                        use_internal: self.use_internal,
                        asset_in: if self.zero_for_one {
                            asset[pair.index0 as usize].addr
                        } else {
                            asset[pair.index1 as usize].addr
                        },
                        asset_out: if !self.zero_for_one {
                            asset[pair.index0 as usize].addr
                        } else {
                            asset[pair.index1 as usize].addr
                        },
                        recipient: self.recipient.unwrap_or_default(),
                        valid_for_block: block,
                        max_amount_in: max_quantity_in,
                        min_amount_in: min_quantity_in,
                        min_price: self.min_price,
                        hook_data: self.hook_data.clone().unwrap_or_default(),
                        max_extra_fee_asset0: self.max_extra_fee_asset0,
                        ..Default::default()
                    };
                    recovered.no_meta_eip712_signing_hash(ANGSTROM_DOMAIN.get().unwrap())
                }
            }
        }
    }
}
