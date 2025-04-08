use std::hash::Hash;

use alloy::primitives::{Address, B256, U256};
use eyre::eyre;
use pade::PadeDecode;
use pade_macro::{PadeDecode, PadeEncode};
use serde::{Deserialize, Serialize};

use crate::{
    contract_payloads::{Asset, Pair, Signature},
    matching::uniswap::{PoolPriceVec, PoolSnapshot, Quantity},
    primitive::ANGSTROM_DOMAIN,
    sol_bindings::{
        RawPoolOrder,
        grouped_orders::OrderWithStorageData,
        rpc_orders::{OmitOrderMeta, TopOfBlockOrder as RpcTopOfBlockOrder}
    }
};

// This currently exists in types::sol_bindings as well, but that one is
// outdated so I'm building a new one here for now and then migrating
#[derive(
    PadeEncode, PadeDecode, Clone, Default, Debug, Hash, PartialEq, Eq, Serialize, Deserialize,
)]
pub struct TopOfBlockOrder {
    pub use_internal:     bool,
    pub quantity_in:      u128,
    pub quantity_out:     u128,
    pub max_gas_asset_0:  u128,
    pub gas_used_asset_0: u128,
    pub pairs_index:      u16,
    pub zero_for_1:       bool,
    pub recipient:        Option<Address>,
    pub signature:        Signature
}

impl TopOfBlockOrder {
    // eip-712 hash_struct. is a pain since we need to reconstruct values.
    fn recover_order(&self, pair: &[Pair], asset: &[Asset], block: u64) -> RpcTopOfBlockOrder {
        let pair = &pair[self.pairs_index as usize];
        RpcTopOfBlockOrder {
            quantity_in:     self.quantity_in,
            recipient:       self.recipient.unwrap_or_default(),
            quantity_out:    self.quantity_out,
            asset_in:        if self.zero_for_1 {
                asset[pair.index0 as usize].addr
            } else {
                asset[pair.index1 as usize].addr
            },
            asset_out:       if !self.zero_for_1 {
                asset[pair.index0 as usize].addr
            } else {
                asset[pair.index1 as usize].addr
            },
            use_internal:    self.use_internal,
            max_gas_asset0:  self.max_gas_asset_0,
            valid_for_block: block,
            meta:            Default::default()
        }
    }

    pub fn order_hash(&self, pair: &[Pair], asset: &[Asset], block: u64) -> B256 {
        let mut order = self.recover_order(pair, asset, block);
        let from = self
            .signature
            .recover_signer(self.signing_hash(pair, asset, block));
        order.meta.from = from;
        order.order_hash()
    }

    pub fn signing_hash(&self, pair: &[Pair], asset: &[Asset], block: u64) -> B256 {
        let order = self.recover_order(pair, asset, block);
        order.no_meta_eip712_signing_hash(&ANGSTROM_DOMAIN)
    }

    pub fn of_max_gas(
        internal: &OrderWithStorageData<RpcTopOfBlockOrder>,
        pairs_index: u16
    ) -> Self {
        assert!(internal.is_valid_signature());
        let quantity_in = internal.quantity_in;
        let quantity_out = internal.quantity_out;
        let recipient = Some(internal.recipient);
        // Zero_for_1 is an Ask, an Ask is NOT a bid
        let zero_for_1 = !internal.is_bid;
        let sig_bytes = internal.meta.signature.to_vec();
        let decoded_signature =
            alloy::primitives::PrimitiveSignature::pade_decode(&mut sig_bytes.as_slice(), None)
                .unwrap();
        let signature = Signature::from(decoded_signature);
        Self {
            use_internal: internal.use_internal,
            quantity_in,
            quantity_out,
            max_gas_asset_0: internal.max_gas_asset0,
            // set as max so we can use the sim to verify values.
            gas_used_asset_0: internal.priority_data.gas.to(),
            pairs_index,
            zero_for_1,
            recipient,
            signature
        }
    }

    pub fn of(
        internal: &OrderWithStorageData<RpcTopOfBlockOrder>,
        _shared_gas: U256,
        pairs_index: u16
    ) -> eyre::Result<Self> {
        let quantity_in = internal.quantity_in;
        let quantity_out = internal.quantity_out;
        let recipient = Some(internal.recipient);
        // Zero_for_1 is an Ask, an Ask is NOT a bid
        let zero_for_1 = !internal.is_bid;
        let sig_bytes = internal.meta.signature.to_vec();
        let decoded_signature =
            alloy::primitives::PrimitiveSignature::pade_decode(&mut sig_bytes.as_slice(), None)
                .unwrap();
        let signature = Signature::from(decoded_signature);
        let used_gas: u128 = (internal.priority_data.gas).saturating_to();

        if used_gas > internal.max_gas_asset0 {
            return Err(eyre::eyre!("order went over gas limit"));
        }

        Ok(Self {
            use_internal: internal.use_internal,
            quantity_in,
            quantity_out,
            max_gas_asset_0: internal.max_gas_asset0,
            // set as max so we can use the sim to verify values.
            gas_used_asset_0: used_gas,
            pairs_index,
            zero_for_1,
            recipient,
            signature
        })
    }

    pub fn calc_vec_and_reward<'a>(
        tob: &OrderWithStorageData<RpcTopOfBlockOrder>,
        snapshot: &'a PoolSnapshot
    ) -> eyre::Result<(PoolPriceVec<'a>, u128)> {
        // First let's simulate the actual ToB swap and use that to determine what our
        // leftover T0 is for rewards
        if tob.is_bid {
            // If ToB is a bid, it's buying T0.  To reward, it will offer in more T1
            // than needed, but the entire input will be swapped through the AMM.
            // Therefore, our input quantity is simple - the entire input amount from
            // the order.
            let pricevec = (snapshot.current_price() + Quantity::Token1(tob.quantity_in))?;

            let leftover = pricevec
                .d_t0
                .checked_sub(tob.quantity_out)
                .ok_or_else(|| eyre!("Not enough output to cover the transaction"))?;
            Ok((pricevec, leftover))
        } else {
            // If ToB is an Ask, it's inputting T0.  We will take the reward T0 first
            // before swapping the remaining T0 with the AMM, so we need to determine
            // how much T0 will actually get to the AMM.  To do this, we determine how
            // much T0 is required to produce the quantity of T1 the order expects to
            // receive as output.  This quantity is our input which moves the AMM.

            // First we find the amount of T0 in it would take to at least hit our quantity
            // out
            let cost = (snapshot.current_price() - Quantity::Token1(tob.quantity_out))?.d_t0;

            let leftover = tob
                .quantity_in
                .checked_sub(cost)
                .ok_or_else(|| eyre!("Not enough input to cover the transaction"))?;

            // But then we have to operate in the right direction to calculate how much T1
            // we ACTUALLY get out
            let pricevec = (snapshot.current_price() + Quantity::Token0(cost))?;
            Ok((pricevec, leftover))
        }
    }
}

#[cfg(test)]
mod test {
    use angstrom_types::matching::{
        SqrtPriceX96,
        uniswap::{LiqRange, PoolSnapshot}
    };
    use uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick;

    fn _generate_amm_market(target_tick: i32) -> PoolSnapshot {
        let range = LiqRange::new_init(target_tick - 1000, target_tick + 1000, 100_000_000_000_000)
            .unwrap();
        let ranges = vec![range];
        let sqrt_price_x96 = SqrtPriceX96::from(get_sqrt_ratio_at_tick(target_tick).unwrap());
        PoolSnapshot::new(10, ranges, sqrt_price_x96).unwrap()
    }

    // #[test]
    // fn calculates_reward() {
    //     let mut rng = thread_rng();
    //     let snapshot = generate_amm_market(100000);
    //     let total_payment = 10_000_000_000_000_u128;
    //     let tob = generate_top_of_block_order(
    //         &mut rng,
    //         true,
    //         None,
    //         None,
    //         Some(total_payment),
    //         Some(100000000_u128)
    //     );
    //     let (res_vec, to_donate) = TopOfBlockOrder::calc_vec_and_reward(&tob,
    // &snapshot)         .expect("Error building base reward data");
    //     let donations = res_vec.t0_donation_to_end_price(to_donate);
    //     assert_eq!(
    //         donations.total_donated + donations.tribute + res_vec.d_t0,
    //         total_payment,
    //         "Total allocations do not add up to input payment"
    //     );
    // }

    //     #[test]
    //     fn handles_insufficient_funds() {
    //         let mut rng = thread_rng();
    //         let snapshot = generate_amm_market(100000);
    //         let tob = generate_top_of_block_order(
    //             &mut rng,
    //             true,
    //             None,
    //             None,
    //             Some(10_000_000_u128),
    //             Some(100000000_u128)
    //         );
    //         let result = calculate_reward(&tob, &snapshot);
    //         assert!(result.is_err_and(|e| {
    //             e.to_string()
    //                 .starts_with("Not enough input to cover the transaction")
    //         }));
    //     }

    //     #[test]
    //     fn handles_precisely_zero_donation() {
    //         let mut rng = thread_rng();
    //         let snapshot = generate_amm_market(100000);
    //         // Hand-calculated that this is the correct payment for this
    // starting price and         // liquidity
    //         let total_payment = 2_201_872_310_000_u128;
    //         let tob = generate_top_of_block_order(
    //             &mut rng,
    //             true,
    //             None,
    //             None,
    //             Some(total_payment),
    //             Some(100000000_u128)
    //         );
    //         let result = calculate_reward(&tob, &snapshot).expect("Error
    // calculating tick donations");         let total_donations =
    // result.total_donations();         assert!(
    //             result.tick_donations.is_empty(),
    //             "Donations are being offered when we shouldn't have any"
    //         );
    //         assert_eq!(
    //             total_donations + result.total_cost + result.tribute,
    //             total_payment,
    //             "Total allocations do not add up to input payment"
    //         );
    //     }

    //     #[test]
    //     fn handles_partial_donation() {
    //         let mut rng = thread_rng();
    //         let snapshot = generate_amm_market(100000);
    //         let partial_donation = 20_000_000_u128;
    //         let total_payment = 2_201_872_310_000_u128 + partial_donation;
    //         let tob = generate_top_of_block_order(
    //             &mut rng,
    //             true,
    //             None,
    //             None,
    //             Some(total_payment),
    //             Some(100000000_u128)
    //         );
    //         let result = calculate_reward(&tob, &snapshot).expect("Error
    // calculating tick donations");         let total_donations =
    // result.total_donations();         assert_eq!(result.tick_donations.len(),
    // 1, "Wrong number of donations");
    // assert!(result.tick_donations. contains_key(&(99000, 101000)),
    // "Donation missing");         assert_eq!(
    // *result.tick_donations.get(&(99000, 101000)).unwrap(),
    // partial_donation,             "Donation of incorrect size"
    //         );
    //         assert_eq!(
    //             total_donations + result.total_cost + result.tribute,
    //             total_payment,
    //             "Total allocations do not add up to input payment"
    //         );
    //     }

    //     #[test]
    //     fn handles_bid_order() {
    //         let mut rng = thread_rng();
    //         let snapshot = generate_amm_market(100000);
    //         let tob = generate_top_of_block_order(
    //             &mut rng,
    //             true,
    //             None,
    //             None,
    //             Some(10_000_000_000_000_u128),
    //             Some(100000000_u128)
    //         );
    //         let result = calculate_reward(&tob, &snapshot);
    //         assert!(result.is_ok());
    //     }

    //     #[test]
    //     fn handles_ask_order() {
    //         let mut rng = thread_rng();
    //         let snapshot = generate_amm_market(100000);
    //         let tob = generate_top_of_block_order(
    //             &mut rng,
    //             false,
    //             None,
    //             None,
    //             Some(10_000_000_000_000_u128),
    //             Some(800000000_u128)
    //         );
    //         let result = calculate_reward(&tob, &snapshot);
    //         assert!(result.is_ok());
    //     }

    //     #[test]
    //     fn only_rewards_initialized_ticks() {
    //         let mut rng = thread_rng();
    //         let snapshot = generate_amm_market(100000);
    //         let total_payment = 2_203_371_417_593_u128;
    //         let tob = generate_top_of_block_order(
    //             &mut rng,
    //             true,
    //             None,
    //             None,
    //             Some(total_payment),
    //             Some(100000000_u128)
    //         );
    //         let first_tick = 100000 - 1000;
    //         let last_tick = 100000 + 1000;
    //         let result = calculate_reward(&tob, &snapshot).expect("Error
    // calculating tick donations");         assert!(
    //             result.tick_donations.len() == 1,
    //             "Too many donations - only one initialized tick in this
    // market"         );
    //         assert!(
    //             result.tick_donations.contains_key(&(first_tick, last_tick)),
    //             "Donation not made to only initialized tick"
    //         );
    //     }
}
