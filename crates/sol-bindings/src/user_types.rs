use alloy_primitives::{Address, Bytes, U256};

use crate::sol::{
    InvalidSolEnumVariant, SolAssetForm, SolFlashOrder, SolOrderMode, SolStandingOrder,
    SolTopOfBlockOrder
};

#[derive(Default, Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct HookData {
    pub hook:    Address,
    pub payload: Bytes
}

#[derive(Default, Debug, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum OrderMode {
    ExactIn,
    ExactOut,
    #[default]
    Partial
}

impl From<OrderMode> for SolOrderMode {
    fn from(value: OrderMode) -> Self {
        match value {
            OrderMode::ExactIn => Self::ExactIn,
            OrderMode::ExactOut => Self::ExactOut,
            OrderMode::Partial => Self::Partial
        }
    }
}

impl TryFrom<SolOrderMode> for OrderMode {
    type Error = InvalidSolEnumVariant;

    fn try_from(value: SolOrderMode) -> Result<Self, Self::Error> {
        match value {
            SolOrderMode::ExactIn => Ok(Self::ExactIn),
            SolOrderMode::ExactOut => Ok(Self::ExactOut),
            SolOrderMode::Partial => Ok(Self::Partial),
            SolOrderMode::__Invalid => Err(InvalidSolEnumVariant)
        }
    }
}

impl From<OrderMode> for &str {
    fn from(val: OrderMode) -> &'static str {
        match val {
            OrderMode::ExactIn => "ExactIn",
            OrderMode::ExactOut => "ExactOut",
            OrderMode::Partial => "Partial"
        }
    }
}

#[derive(Default, Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum AssetForm {
    #[default]
    Liquid,
    UniV4Claim,
    AngstromClaim
}

impl From<AssetForm> for SolAssetForm {
    fn from(value: AssetForm) -> Self {
        match value {
            AssetForm::Liquid => Self::Liquid,
            AssetForm::UniV4Claim => Self::UniV4Claim,
            AssetForm::AngstromClaim => Self::AngstromClaim
        }
    }
}

impl TryFrom<SolAssetForm> for AssetForm {
    type Error = InvalidSolEnumVariant;

    fn try_from(value: SolAssetForm) -> Result<Self, Self::Error> {
        match value {
            SolAssetForm::Liquid => Ok(Self::Liquid),
            SolAssetForm::UniV4Claim => Ok(Self::UniV4Claim),
            SolAssetForm::AngstromClaim => Ok(Self::AngstromClaim),
            SolAssetForm::__Invalid => Err(InvalidSolEnumVariant)
        }
    }
}

pub fn hook_data_to_bytes(hook_data: Option<&HookData>) -> Bytes {
    hook_data
        .map(|inner_hook_data| {
            let mut bytes = Vec::with_capacity(inner_hook_data.payload.len() + 20);
            bytes.extend_from_slice(inner_hook_data.hook.as_slice());
            bytes.extend_from_slice(&inner_hook_data.payload);
            bytes.into()
        })
        .unwrap_or_default()
}

#[derive(Default, Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct StandingOrder {
    pub mode:                 OrderMode,
    pub max_amount_in_or_out: U256,
    pub min_price:            U256,
    pub asset_in:             Address,
    pub asset_in_form:        AssetForm,
    pub asset_out:            Address,
    pub asset_out_form:       AssetForm,
    pub recipient:            Address,
    pub hook_data:            Option<HookData>,
    pub nonce:                u64,
    pub deadline:             U256
}

impl From<StandingOrder> for SolStandingOrder {
    fn from(value: StandingOrder) -> Self {
        Self {
            mode:                 Into::<&str>::into(value.mode).to_owned(),
            max_amount_in_or_out: value.max_amount_in_or_out,
            min_price:            value.min_price,
            asset_in:             value.asset_in,
            asset_in_form:        value.asset_in_form.into(),
            asset_out:            value.asset_out,
            asset_out_form:       value.asset_out_form.into(),
            recipient:            value.recipient,
            hook_data:            hook_data_to_bytes(value.hook_data.as_ref()),
            nonce:                value.nonce,
            deadline:             value.deadline
        }
    }
}

#[derive(Debug, Default, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct FlashOrder {
    pub mode:                 OrderMode,
    pub max_amount_in_or_out: U256,
    pub min_price:            U256,
    pub asset_in:             Address,
    pub asset_in_form:        AssetForm,
    pub asset_out:            Address,
    pub asset_out_form:       AssetForm,
    pub recipient:            Address,
    pub hook_data:            Option<HookData>,
    pub valid_for_block:      u64
}

impl From<FlashOrder> for SolFlashOrder {
    fn from(value: FlashOrder) -> Self {
        Self {
            mode:                 Into::<&str>::into(value.mode).to_owned(),
            max_amount_in_or_out: value.max_amount_in_or_out,
            min_price:            value.min_price,
            asset_in:             value.asset_in,
            asset_in_form:        value.asset_in_form.into(),
            asset_out:            value.asset_out,
            asset_out_form:       value.asset_out_form.into(),
            recipient:            value.recipient,
            hook_data:            hook_data_to_bytes(value.hook_data.as_ref()),
            valid_for_block:      value.valid_for_block
        }
    }
}

#[derive(Debug, Default, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct TopOfBlockOrder {
    pub amount_in:       U256,
    pub amount_out:      U256,
    pub asset_in:        Address,
    pub asset_in_form:   AssetForm,
    pub asset_out:       Address,
    pub asset_out_form:  AssetForm,
    pub recipient:       Address,
    pub hook_data:       Option<HookData>,
    pub valid_for_block: u64
}

impl From<TopOfBlockOrder> for SolTopOfBlockOrder {
    fn from(value: TopOfBlockOrder) -> Self {
        Self {
            amount_in:       value.amount_in,
            amount_out:      value.amount_out,
            asset_in:        value.asset_in,
            asset_in_form:   value.asset_in_form.into(),
            asset_out:       value.asset_out,
            asset_out_form:  value.asset_out_form.into(),
            recipient:       value.recipient,
            hook_data:       hook_data_to_bytes(value.hook_data.as_ref()),
            valid_for_block: value.valid_for_block
        }
    }
}
