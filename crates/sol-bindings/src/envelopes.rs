use crate::sol::{SolGenericOrder, SolOrderType, SolTopOfBlockOrderEnvelope};
use crate::user_types::{FlashOrder, HookData, StandingOrder, TopOfBlockOrder};
use alloy_primitives::{Address, Bytes, U256};
use alloy_sol_types::SolStruct;

pub trait IntoEnvelope {
    type E: SolStruct;

    fn into_envelope(
        self,
        sorted_asset_list: &[Address],
        from: Address,
        signature: Bytes,
        amount_filled: U256,
    ) -> Option<Self::E>;
}

fn addr_to_asset_index(sorted_asset_list: &[Address], addr: &Address) -> Option<u16> {
    sorted_asset_list.binary_search(addr).ok()?.try_into().ok()
}

impl IntoEnvelope for FlashOrder {
    type E = SolGenericOrder;

    fn into_envelope(
        self,
        sorted_asset_list: &[Address],
        from: Address,
        signature: Bytes,
        amount_filled: U256,
    ) -> Option<Self::E> {
        let HookData {
            hook,
            payload: hook_payload,
        } = self.hook_data.unwrap_or_default();
        Some(SolGenericOrder {
            otype: SolOrderType::Flash,
            mode: self.mode.into(),
            amountSpecified: self.max_amount_in_or_out,
            minPrice: self.min_price,
            assetInIndex: addr_to_asset_index(sorted_asset_list, &self.asset_in)?,
            assetInForm: self.asset_in_form.into(),
            assetOutIndex: addr_to_asset_index(sorted_asset_list, &self.asset_out)?,
            assetOutForm: self.asset_out_form.into(),
            nonce: Default::default(),
            deadline: Default::default(),
            recipient: self.recipient,
            hook,
            hookPayload: hook_payload,
            amountFilled: amount_filled,
            from,
            signature,
        })
    }
}

impl IntoEnvelope for StandingOrder {
    type E = SolGenericOrder;

    fn into_envelope(
        self,
        sorted_asset_list: &[Address],
        from: Address,
        signature: Bytes,
        amount_filled: U256,
    ) -> Option<Self::E> {
        let HookData {
            hook,
            payload: hook_payload,
        } = self.hook_data.unwrap_or_default();
        Some(SolGenericOrder {
            otype: SolOrderType::Flash,
            mode: self.mode.into(),
            amountSpecified: self.max_amount_in_or_out,
            minPrice: self.min_price,
            assetInIndex: addr_to_asset_index(sorted_asset_list, &self.asset_in)?,
            assetInForm: self.asset_in_form.into(),
            assetOutIndex: addr_to_asset_index(sorted_asset_list, &self.asset_out)?,
            assetOutForm: self.asset_out_form.into(),
            nonce: self.nonce,
            deadline: self.deadline,
            recipient: self.recipient,
            hook,
            hookPayload: hook_payload,
            amountFilled: amount_filled,
            from,
            signature,
        })
    }
}

impl IntoEnvelope for TopOfBlockOrder {
    type E = SolTopOfBlockOrderEnvelope;

    fn into_envelope(
        self,
        sorted_asset_list: &[Address],
        from: Address,
        signature: Bytes,
        _amount_filled: U256,
    ) -> Option<Self::E> {
        let HookData {
            hook,
            payload: hook_payload,
        } = self.hook_data.unwrap_or_default();
        Some(SolTopOfBlockOrderEnvelope {
            amountIn: self.amount_in,
            amountOut: self.amount_out,
            assetInIndex: addr_to_asset_index(sorted_asset_list, &self.asset_in)?,
            assetInForm: self.asset_in_form.into(),
            assetOutIndex: addr_to_asset_index(sorted_asset_list, &self.asset_out)?,
            assetOutForm: self.asset_out_form.into(),
            recipient: self.recipient,
            hook,
            hookPayload: hook_payload,
            from,
            signature,
        })
    }
}
