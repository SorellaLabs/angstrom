use alloy::primitives::{aliases::U40, Address, Bytes, B256, U256};
use pade::PadeDecode;
use pade_macro::{PadeDecode, PadeEncode};

use crate::{
    contract_payloads::{Asset, Pair, Signature},
    orders::OrderOutcome,
    sol_bindings::{
        grouped_orders::{
            FlashVariants, GroupedVanillaOrder, OrderWithStorageData, StandingVariants
        },
        rpc_orders::{
            ExactFlashOrder, ExactStandingOrder, PartialFlashOrder, PartialStandingOrder
        },
        RawPoolOrder
    }
};

#[derive(Debug, Clone, PadeEncode, PadeDecode)]
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

#[derive(Debug, Clone, PadeEncode, PadeDecode)]
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

#[derive(Debug, Clone, PadeEncode, PadeDecode)]
pub struct UserOrder {
    pub ref_id:               u32,
    pub use_internal:         bool,
    pub pair_index:           u16,
    pub min_price:            alloy::primitives::U256,
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
    pub fn order_hash(&self, pair: &[Pair], asset: &[Asset], block: u64) -> B256 {
        let pair = &pair[self.pair_index as usize];
        match self.order_quantities {
            OrderQuantities::Exact { quantity } => {
                if let Some(validation) = &self.standing_validation {
                    // exact standing
                    ExactStandingOrder {
                        ref_id: self.ref_id,
                        exact_in: true,
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
                    }
                    .order_hash()
                } else {
                    // exact flash
                    ExactFlashOrder {
                        ref_id: self.ref_id,
                        exact_in: true,
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
                    }
                    .order_hash()
                }
            }
            OrderQuantities::Partial { min_quantity_in, max_quantity_in, .. } => {
                if let Some(validation) = &self.standing_validation {
                    PartialStandingOrder {
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
                    }
                    .order_hash()
                } else {
                    PartialFlashOrder {
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
                    }
                    .order_hash()
                }
            }
        }
    }

    pub fn from_internal_order(
        order: &OrderWithStorageData<GroupedVanillaOrder>,
        outcome: &OrderOutcome,
        shared_gas: U256,
        pair_index: u16
    ) -> eyre::Result<Self> {
        let (order_quantities, standing_validation, recipient) = match &order.order {
            GroupedVanillaOrder::KillOrFill(o) => match o {
                FlashVariants::Exact(e) => {
                    (OrderQuantities::Exact { quantity: order.amount_in() }, None, e.recipient)
                }
                FlashVariants::Partial(p_o) => (
                    OrderQuantities::Partial {
                        min_quantity_in: p_o.min_amount_in,
                        max_quantity_in: p_o.max_amount_in,
                        filled_quantity: outcome.fill_amount(p_o.max_amount_in)
                    },
                    None,
                    p_o.recipient
                )
            },
            GroupedVanillaOrder::Standing(o) => match o {
                StandingVariants::Exact(e) => (
                    OrderQuantities::Exact { quantity: order.amount_in() },
                    Some(StandingValidation { nonce: e.nonce, deadline: e.deadline.to() }),
                    e.recipient
                ),
                StandingVariants::Partial(p_o) => {
                    let max_quantity_in = p_o.max_amount_in;
                    let filled_quantity = outcome.fill_amount(p_o.max_amount_in);
                    (
                        OrderQuantities::Partial {
                            min_quantity_in: p_o.min_amount_in,
                            max_quantity_in,
                            filled_quantity
                        },
                        Some(StandingValidation {
                            nonce:    p_o.nonce,
                            deadline: p_o.deadline.to()
                        }),
                        p_o.recipient
                    )
                }
            }
        };
        let hook_bytes = match order.order {
            GroupedVanillaOrder::KillOrFill(ref o) => o.hook_data().clone(),
            GroupedVanillaOrder::Standing(ref o) => o.hook_data().clone()
        };
        let hook_data = if hook_bytes.is_empty() { None } else { Some(hook_bytes) };

        let user = order.from();
        let recipient = (user != recipient).then_some(recipient);
        let gas_used: u128 = (order.priority_data.gas + shared_gas).to();
        if gas_used > order.max_gas_token_0() {
            return Err(eyre::eyre!("order used more gas than allocated"))
        }

        let sig_bytes = order.signature().clone().0.to_vec();
        let decoded_signature =
            alloy::primitives::PrimitiveSignature::pade_decode(&mut sig_bytes.as_slice(), None)
                .unwrap();
        let signature = Signature::from(decoded_signature);

        Ok(Self {
            ref_id: 0,
            use_internal: order.use_internal(),
            pair_index,
            min_price: *order.price(),
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
        order: &OrderWithStorageData<GroupedVanillaOrder>,
        outcome: &OrderOutcome,
        pair_index: u16
    ) -> Self {
        let (order_quantities, standing_validation, recipient) = match &order.order {
            GroupedVanillaOrder::KillOrFill(o) => match o {
                FlashVariants::Exact(e) => {
                    (OrderQuantities::Exact { quantity: order.amount_in() }, None, e.recipient)
                }
                FlashVariants::Partial(p_o) => (
                    OrderQuantities::Partial {
                        min_quantity_in: p_o.min_amount_in,
                        max_quantity_in: p_o.max_amount_in,
                        filled_quantity: outcome.fill_amount(p_o.max_amount_in)
                    },
                    None,
                    p_o.recipient
                )
            },
            GroupedVanillaOrder::Standing(o) => match o {
                StandingVariants::Exact(e) => (
                    OrderQuantities::Exact { quantity: order.amount_in() },
                    Some(StandingValidation { nonce: e.nonce, deadline: e.deadline.to() }),
                    e.recipient
                ),
                StandingVariants::Partial(p_o) => {
                    let max_quantity_in = p_o.max_amount_in;
                    let filled_quantity = outcome.fill_amount(p_o.max_amount_in);
                    (
                        OrderQuantities::Partial {
                            min_quantity_in: p_o.min_amount_in,
                            max_quantity_in,
                            filled_quantity
                        },
                        Some(StandingValidation {
                            nonce:    p_o.nonce,
                            deadline: p_o.deadline.to()
                        }),
                        p_o.recipient
                    )
                }
            }
        };
        let hook_bytes = match order.order {
            GroupedVanillaOrder::KillOrFill(ref o) => o.hook_data().clone(),
            GroupedVanillaOrder::Standing(ref o) => o.hook_data().clone()
        };
        let hook_data = if hook_bytes.is_empty() { None } else { Some(hook_bytes) };
        let sig_bytes = order.signature().to_vec();
        let decoded_signature =
            alloy::primitives::PrimitiveSignature::pade_decode(&mut sig_bytes.as_slice(), None)
                .unwrap();

        let user = order.from();
        let recipient = (user != recipient).then_some(recipient);

        Self {
            ref_id: 0,
            use_internal: order.use_internal(),
            pair_index,
            min_price: *order.price(),
            recipient,
            hook_data,
            zero_for_one: !order.is_bid,
            standing_validation,
            order_quantities,
            max_extra_fee_asset0: order.max_gas_token_0(),
            extra_fee_asset0: order.max_gas_token_0(),
            exact_in: order.exact_in(),
            signature: Signature::from(decoded_signature)
        }
    }
}
