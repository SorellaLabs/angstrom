use std::hash::Hash;

use alloy_primitives::I256;
use angstrom_types_primitives::{
    contract_payloads::{Asset, Pair, Signature, angstrom::TopOfBlockOrder},
    primitive::ANGSTROM_DOMAIN,
    sol_bindings::{
        RawPoolOrder,
        grouped_orders::OrderWithStorageData,
        rpc_orders::{OmitOrderMeta, TopOfBlockOrder as RpcTopOfBlockOrder}
    }
};
use eyre::eyre;
use pade::PadeDecode;

use crate::{
    traits::tob::TopOfBlockOrderRewardCalc,
    uni_structure::{BaselinePoolState, pool_swap::PoolSwapResult}
};
pub trait TopOfBlockOrderRewardCalc: Sized {
    fn calc_vec_and_reward<'a>(
        tob: &OrderWithStorageData<RpcTopOfBlockOrder>,
        snapshot: &'a BaselinePoolState
    ) -> eyre::Result<(PoolSwapResult<'a>, u128)>;

    fn calc_reward(&self, snapshot: BaselinePoolState) -> eyre::Result<u128>;

    fn get_auction_bid(&self, snapshot: &BaselinePoolState) -> eyre::Result<u128>;
}

impl TopOfBlockOrderRewardCalc for TopOfBlockOrder {
    fn calc_vec_and_reward<'a>(
        tob: &OrderWithStorageData<RpcTopOfBlockOrder>,
        snapshot: &'a BaselinePoolState
    ) -> eyre::Result<(PoolSwapResult<'a>, u128)> {
        // First let's simulate the actual ToB swap and use that to determine what our
        // leftover T0 is for rewards
        if tob.is_bid {
            // If ToB is a bid, it's buying T0.  To reward, it will offer in more T1
            // than needed, but the entire input will be swapped through the AMM.
            // Therefore, our input quantity is simple - the entire input amount from
            // the order.
            let res =
                snapshot.swap_current_with_amount(I256::unchecked_from(tob.quantity_in), false)?;
            let leftover = res
                .total_d_t0
                .checked_sub(tob.quantity_out)
                .ok_or_else(|| eyre!("Not enough output to cover the transaction"))?;

            Ok((res, leftover))
        } else {
            // If ToB is an Ask, it's inputting T0.  We will take the reward T0 first
            // before swapping the remaining T0 with the AMM, so we need to determine
            // how much T0 will actually get to the AMM.  To do this, we determine how
            // much T0 is required to produce the quantity of T1 the order expects to
            // receive as output.  This quantity is our input which moves the AMM.

            // First we find the amount of T0 in it would take to at least hit our quantity
            // out

            let cost = snapshot
                .swap_current_with_amount(-I256::unchecked_from(tob.quantity_out), true)?
                .total_d_t0;

            let leftover = tob
                .quantity_in
                .checked_sub(cost)
                .ok_or_else(|| eyre!("Not enough input to cover the transaction"))?;

            let price_vec = snapshot.swap_current_with_amount(I256::unchecked_from(cost), true)?;
            Ok((price_vec, leftover))
        }
    }

    fn calc_reward(&self, snapshot: BaselinePoolState) -> eyre::Result<u128> {
        if !self.zero_for_1 {
            let res =
                snapshot.swap_current_with_amount(I256::unchecked_from(self.quantity_in), false)?;
            res.total_d_t0
                .checked_sub(self.quantity_out)
                .ok_or_else(|| eyre!("Not enough output to cover the transaction"))
        } else {
            let cost = snapshot
                .swap_current_with_amount(-I256::unchecked_from(self.quantity_out), true)?
                .total_d_t0;

            self.quantity_in
                .checked_sub(cost)
                .ok_or_else(|| eyre!("Not enough input to cover the transaction"))
        }
    }

    /// returns the amount in t0 that this order is bidding in the auction.
    fn get_auction_bid(&self, snapshot: &BaselinePoolState) -> eyre::Result<u128> {
        // Cefi Sell
        if self.is_bid() {
            let res =
                snapshot.swap_current_with_amount(I256::unchecked_from(self.quantity_in), false)?;
            res.total_d_t0
                .checked_sub(self.quantity_out)
                .ok_or_else(|| eyre!("Not enough output to cover the transaction"))
        } else {
            // qty am_in -> price
            // Cefi Buy
            let cost = snapshot
                .swap_current_with_amount(-I256::unchecked_from(self.quantity_out), true)?
                .total_d_t0;

            self.quantity_in
                .checked_sub(cost)
                .ok_or_else(|| eyre!("Not enough input to cover the transaction"))
        }
    }
}
