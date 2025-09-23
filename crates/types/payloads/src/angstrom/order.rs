use alloy::primitives::{Address, B256, Bytes, U256, aliases::U40};
use pade_macro::{PadeDecode, PadeEncode};
use serde::{Deserialize, Serialize};

use crate::{
    contract_payloads::{Asset, Pair, Signature},
    orders::OrderOutcome,
    primitive::ANGSTROM_DOMAIN,
    sol_bindings::{
        RawPoolOrder,
        grouped_orders::{AllOrders, OrderWithStorageData},
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

    pub fn from_internal_order(
        order: &OrderWithStorageData<AllOrders>,
        outcome: &OrderOutcome,
        _: U256,
        pair_index: u16
    ) -> eyre::Result<Self> {
        let (order_quantities, standing_validation, recipient) = match &order.order {
            AllOrders::PartialFlash(p_o) => (
                OrderQuantities::Partial {
                    min_quantity_in: p_o.min_amount_in,
                    max_quantity_in: p_o.max_amount_in,
                    filled_quantity: outcome.fill_amount(p_o.max_amount_in)
                },
                None,
                p_o.recipient
            ),
            AllOrders::PartialStanding(p_o) => (
                OrderQuantities::Partial {
                    min_quantity_in: p_o.min_amount_in,
                    max_quantity_in: p_o.max_amount_in,
                    filled_quantity: outcome.fill_amount(p_o.max_amount_in)
                },
                Some(StandingValidation { nonce: p_o.nonce, deadline: p_o.deadline.to() }),
                p_o.recipient
            ),
            AllOrders::ExactFlash(o) => {
                (OrderQuantities::Exact { quantity: order.amount() }, None, o.recipient)
            }
            AllOrders::ExactStanding(o) => (
                OrderQuantities::Exact { quantity: order.amount() },
                Some(StandingValidation { nonce: o.nonce, deadline: o.deadline.to() }),
                o.recipient
            ),
            AllOrders::TOB(_) => panic!("tob found in limit orders")
        };
        let hook_bytes = match &order.order {
            AllOrders::PartialFlash(p_o) => p_o.hook_data.clone(),
            AllOrders::PartialStanding(p_o) => p_o.hook_data.clone(),
            AllOrders::ExactFlash(o) => o.hook_data.clone(),
            AllOrders::ExactStanding(o) => o.hook_data.clone(),
            AllOrders::TOB(_) => panic!("tob found in limit orders")
        };
        let hook_data = if hook_bytes.is_empty() { None } else { Some(hook_bytes) };

        let recipient = (!recipient.is_zero()).then_some(recipient);
        let gas_used: u128 = (order.priority_data.gas).to();
        if gas_used > order.max_gas_token_0() {
            return Err(eyre::eyre!("order used more gas than allocated"));
        }

        let sig = order.order_signature().unwrap();
        let signature = Signature::from(sig);

        Ok(Self {
            ref_id: 0,
            use_internal: order.use_internal(),
            pair_index,
            min_price: order.limit_price(),
            recipient,
            hook_data,
            zero_for_one: !order.is_bid,
            standing_validation,
            order_quantities,
            max_extra_fee_asset0: order.max_gas_token_0(),
            extra_fee_asset0: gas_used,
            exact_in: order.exact_in(),
            signature
        })
    }

    pub fn from_internal_order_max_gas(
        order: &OrderWithStorageData<AllOrders>,
        outcome: &OrderOutcome,
        pair_index: u16
    ) -> Self {
        let (order_quantities, standing_validation, recipient) = match &order.order {
            AllOrders::PartialFlash(p_o) => (
                OrderQuantities::Partial {
                    min_quantity_in: p_o.min_amount_in,
                    max_quantity_in: p_o.max_amount_in,
                    filled_quantity: outcome.fill_amount(p_o.max_amount_in)
                },
                None,
                p_o.recipient
            ),
            AllOrders::PartialStanding(p_o) => (
                OrderQuantities::Partial {
                    min_quantity_in: p_o.min_amount_in,
                    max_quantity_in: p_o.max_amount_in,
                    filled_quantity: outcome.fill_amount(p_o.max_amount_in)
                },
                Some(StandingValidation { nonce: p_o.nonce, deadline: p_o.deadline.to() }),
                p_o.recipient
            ),
            AllOrders::ExactFlash(o) => {
                (OrderQuantities::Exact { quantity: order.amount() }, None, o.recipient)
            }
            AllOrders::ExactStanding(o) => (
                OrderQuantities::Exact { quantity: order.amount() },
                Some(StandingValidation { nonce: o.nonce, deadline: o.deadline.to() }),
                o.recipient
            ),
            AllOrders::TOB(_) => panic!("tob found in limit orders")
        };
        let hook_bytes = match &order.order {
            AllOrders::PartialFlash(p_o) => p_o.hook_data.clone(),
            AllOrders::PartialStanding(p_o) => p_o.hook_data.clone(),
            AllOrders::ExactFlash(o) => o.hook_data.clone(),
            AllOrders::ExactStanding(o) => o.hook_data.clone(),
            AllOrders::TOB(_) => panic!("tob found in limit orders")
        };
        let hook_data = if hook_bytes.is_empty() { None } else { Some(hook_bytes) };
        let sig = order.order_signature().unwrap();
        let signature = Signature::from(sig);

        let recipient = (!recipient.is_zero()).then_some(recipient);

        Self {
            ref_id: 0,
            use_internal: order.use_internal(),
            pair_index,
            min_price: order.limit_price(),
            recipient,
            hook_data,
            zero_for_one: !order.is_bid,
            standing_validation,
            order_quantities,
            max_extra_fee_asset0: order.max_gas_token_0(),
            extra_fee_asset0: order.priority_data.gas.to(),
            exact_in: order.exact_in(),
            signature
        }
    }
}
