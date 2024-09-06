use std::collections::HashMap;

use alloy::primitives::{I256, U256};
use angstrom_types::{
    matching::{Ray, SqrtPriceX96},
    sol_bindings::{
        grouped_orders::OrderWithStorageData,
        sol::{SolPoolRewardsUpdate, SolRewardsUpdate, TopOfBlockOrder}
    }
};
use eyre::{eyre, Context, OptionExt};
use uniswap_v3_math::{swap_math::compute_swap_step, tick_math::get_sqrt_ratio_at_tick};

use super::{MarketSnapshot, Tick};

#[derive(Debug)]
pub struct ToBOutcome {
    pub start_tick:      i32,
    pub start_liquidity: u128,
    pub tribute:         U256,
    pub total_cost:      U256,
    pub tick_donations:  HashMap<Tick, U256>
}

impl ToBOutcome {
    /// Sum of the donations across all ticks
    pub fn total_donations(&self) -> U256 {
        self.tick_donations
            .iter()
            .fold(U256::ZERO, |acc, (_tick, donation)| acc + donation)
    }

    /// Tick donations plus tribute to determine total value of this outcome
    pub fn total_value(&self) -> U256 {
        self.total_donations() + self.tribute
    }

    pub fn to_donate(&self, a0_idx: u16, a1_idx: u16) -> SolPoolRewardsUpdate {
        let mut donations = self.tick_donations.iter().collect::<Vec<_>>();
        // Will sort from lowest to highest (donations[0] will be the lowest tick
        // number)
        donations.sort_by_key(|f| f.0);
        // Each reward value is the cumulative sum of the rewards before it
        let quantities = donations
            .iter()
            .scan(U256::ZERO, |state, (_tick, q)| {
                *state += **q;
                Some(u128::try_from(*state).unwrap())
            })
            .collect::<Vec<_>>();
        let update = SolRewardsUpdate {
            startTick: *donations[0].0 + 1,
            startLiquidity: self.start_liquidity,
            quantities
        };
        SolPoolRewardsUpdate { asset0: a0_idx, asset1: a1_idx, update }
    }
}

pub fn calculate_reward(
    tob: OrderWithStorageData<TopOfBlockOrder>,
    amm: MarketSnapshot
) -> eyre::Result<ToBOutcome> {
    // This implies that a bid will be purchasing T0 out of the pool, therefore
    // increasing the price while an ask will be selling T0 to the pool, decreasing
    // the price
    let tick_motion = if tob.is_bid { 1 } else { -1 };

    // We start out at the tick and price that the AMM begins at
    let mut current_tick = amm.current_tick;
    let mut current_price = amm.sqrt_price_x96;
    // TODO:  Figure out how fee pips factor into this
    let fee_pips = 600;

    // Turn our output into a negative number so compute_swap_step knows we're
    // looking to get an exact amount out
    let mut expected_out = I256::try_from(tob.order.amountOut).wrap_err_with(|| {
        format!(
            "Expected ToB order output too large to convert U256 -> I256: {}",
            tob.order.amountOut
        )
    })? * I256::MINUS_ONE;

    // Initialize some things we're going to do to track
    let mut total_cost = U256::ZERO;
    let mut stakes = Vec::new();

    // The bid/ask direction determines what we're trading.  In all cases, our
    // amountIn is what we have to give and our amountOut is what we expect to get.
    // So our bribe is always housed in amountIn no matter what the directionality
    // of the order is, and we always want to count down our amountOut to find out
    // where we stop selling to the AMM and start taking a bribe
    while expected_out < I256::ZERO {
        let next_tick = current_tick + tick_motion;
        let next_price = SqrtPriceX96::from(
            get_sqrt_ratio_at_tick(next_tick)
                .wrap_err_with(|| format!("Unable to get SqrtPrice at tick {}", next_tick))?
        );
        let liquidity = amm
            .liquidity_at_tick(current_tick)
            .ok_or_else(|| eyre!("Unable to find liquidity for tick {}", current_tick))?;
        let (fin_price, amount_in, amount_out, amount_fee) = compute_swap_step(
            current_price.into(),
            next_price.into(),
            liquidity,
            expected_out,
            fee_pips
        )
        .wrap_err_with(|| {
            format!("Unable to compute swap step from tick {} to {}", current_tick, next_tick)
        })?;

        // See how much output we have yet to go
        let signed_out = I256::try_from(amount_out)
            .wrap_err("Output of step too large to convert U256 -> I256")?;
        expected_out = expected_out
            .checked_add(signed_out)
            .ok_or_eyre("Unable to add signed_out to expected_out")?;

        // Add the amount in and our total fee to our cost
        total_cost += amount_in;
        total_cost += amount_fee;

        // How much should this have cost if it was done by the raw price
        let end_price = Ray::from(SqrtPriceX96::from(fin_price));

        // This seems to work properly, so let's run with it
        let avg_price = Ray::calc_price(amount_in, amount_out);

        // See if we have enough bribe left over to cover the total amount so far (can
        // we do this)?
        stakes.push((avg_price, end_price, amount_out));

        // Iterate!
        current_tick += tick_motion;
        current_price = SqrtPriceX96::from(fin_price);
    }

    // Determine how much extra amountIn we have that will be used as tribute to the
    // LPs
    let bribe = tob.amountIn.checked_sub(total_cost).ok_or_else(|| {
        eyre!("Total cost greater than amount offered: {} > {}", total_cost, tob.amountIn)
    })?;

    if stakes.is_empty() {
        // TODO: Maybe this should just be a big donation to the current tick?
        return Err(eyre!("No actual purchases could be made with this TOB order"));
    }

    let mut rem_bribe = bribe;
    let mut cur_q = U256::ZERO;
    let mut filled_price = stakes[0].0;

    let mut stake_iter = stakes.iter().peekable();
    while let Some(stake) = stake_iter.next() {
        let q_step = cur_q + stake.2;
        // Our target price is either the average price of the next stake or the end
        // price of the current stake if there's no next stake to deal with
        let target_price = stake_iter
            .peek()
            .map(|next_stake| next_stake.0)
            .unwrap_or_else(|| stake.1);
        // The difference between this tick's average price and our target price
        let d_price = target_price - stake.0;

        // The step cost is the total cost in needed to ensure that all sold quantities
        // were sold at our target price
        let step_cost = d_price.mul_quantity(q_step);

        if rem_bribe >= step_cost {
            // If we have enough bribe to pay the whole cost, allocate that and step forward
            // to the next price gap
            cur_q += stake.2;
            filled_price = target_price;
            rem_bribe -= step_cost;
        } else {
            // If we don't have enough bribe to pay the whole cost, figure out where the
            // target price winds up based on what we do have and end this iteration
            let partial_dprice = Ray::calc_price(rem_bribe, q_step);
            filled_price += partial_dprice;
            break;
        }
    }

    // We've now found our filled price, we can allocate our reward to each tick
    // based on how much it costs to bring them up to that price.
    let mut reward_t = U256::ZERO;
    let tick_donations: HashMap<Tick, U256> = stakes
        .iter()
        .enumerate()
        .filter_map(|(i, stake)| {
            let tick_num = amm.current_tick + (i as i32 * tick_motion);
            if filled_price > stake.0 {
                let total_dprice = filled_price - stake.0;
                let total_reward = total_dprice.mul_quantity(stake.2);
                if total_reward > U256::ZERO {
                    reward_t += total_reward;
                    Some((tick_num, total_reward))
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();
    let tribute = bribe - reward_t;
    // Both our tribute and our tick_donations are done in the same currency as
    // amountIn
    Ok(ToBOutcome {
        start_tick: amm.current_tick,
        start_liquidity: amm.current_position().liquidity(),
        tribute,
        total_cost,
        tick_donations
    })
}

#[cfg(test)]
mod test {
    use alloy::{
        primitives::{address, Bytes, Uint, U256},
        providers::{ext::AnvilApi, Provider, ProviderBuilder},
        rpc::types::Filter
    };
    use angstrom_types::{
        contract_bindings::{
            mockrewardsmanager::MockRewardsManager::{MockRewardsManagerInstance, PoolId},
            poolmanager::PoolManager
        },
        contract_payloads::tob::{Asset, MockContractMessage, PoolRewardsUpdate, RewardsUpdate},
        matching::SqrtPriceX96
    };
    use rand::thread_rng;
    use testing_tools::type_generator::orders::generate_top_of_block_order;
    use uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick;

    use super::calculate_reward;
    use crate::cfmm::uniswap::{MarketSnapshot, PoolRange};

    fn generate_amm_market(target_tick: i32) -> MarketSnapshot {
        let range =
            PoolRange::new(target_tick - 1000, target_tick + 1000, 100_000_000_000_000).unwrap();
        let ranges = vec![range];
        let sqrt_price_x96 = SqrtPriceX96::from(get_sqrt_ratio_at_tick(target_tick).unwrap());
        MarketSnapshot::new(ranges, sqrt_price_x96).unwrap()
    }

    #[test]
    fn calculates_reward() {
        let mut rng = thread_rng();
        let amm = generate_amm_market(100000);
        let mut order = generate_top_of_block_order(&mut rng, true, None, None);
        let total_payment = Uint::from(10_000_000_000_000_u128);
        order.order.amountIn = total_payment;
        order.order.amountOut = Uint::from(100000000);
        let result = calculate_reward(order, amm).expect("Error calculating tick donations");
        let total_donations = result.total_donations();
        assert_eq!(
            total_donations + result.total_cost + result.tribute,
            total_payment,
            "Total allocations do not add up to input payment"
        );
    }

    #[test]
    fn handles_insufficient_funds() {
        let mut rng = thread_rng();
        let amm = generate_amm_market(-100000);
        let mut order = generate_top_of_block_order(&mut rng, true, None, None);
        order.is_bid = true;
        order.order.amountOut = Uint::from(10_000_000_000_000_u128);
        order.order.amountIn = Uint::from(100000000);
        let result = calculate_reward(order, amm);
        assert!(result.is_err_and(|e| {
            e.to_string()
                .starts_with("Total cost greater than amount offered")
        }))
    }

    #[test]
    fn handles_precisely_zero_donation() {
        let mut rng = thread_rng();
        let amm = generate_amm_market(100000);
        let mut order = generate_top_of_block_order(&mut rng, true, None, None);
        let total_payment = Uint::from(2_203_194_246_001_u128);
        order.order.amountIn = total_payment;
        order.order.amountOut = Uint::from(100000000);
        let result = calculate_reward(order, amm).expect("Error calculating tick donations");
        let total_donations = result.total_donations();
        assert!(
            result.tick_donations.is_empty(),
            "Donations are being offered when we shouldn't have any"
        );
        assert_eq!(
            total_donations + result.total_cost + result.tribute,
            total_payment,
            "Total allocations do not add up to input payment"
        );
    }

    #[test]
    fn handles_partial_donation() {
        let mut rng = thread_rng();
        let amm = generate_amm_market(100000);
        let mut order = generate_top_of_block_order(&mut rng, true, None, None);
        let total_payment = Uint::from(2_203_371_417_593_u128);
        order.order.amountIn = total_payment;
        order.order.amountOut = Uint::from(100000000);
        let result = calculate_reward(order, amm).expect("Error calculating tick donations");
        let total_donations = result.total_donations();
        assert!(result.tick_donations.contains_key(&100000), "Donation to first tick missing");
        assert!(result.tick_donations.contains_key(&100001), "Donation to second tick missing");
        assert!(
            !result.tick_donations.contains_key(&100002),
            "Donation to third tick present when it shouldn't be"
        );
        assert_eq!(
            total_donations + result.total_cost + result.tribute,
            total_payment,
            "Total allocations do not add up to input payment"
        );
    }

    #[test]
    fn handles_bid_order() {
        let mut rng = thread_rng();
        let amm = generate_amm_market(100000);
        let mut order = generate_top_of_block_order(&mut rng, true, None, None);
        order.is_bid = true;
        order.order.amountIn = Uint::from(10_000_000_000_000_u128);
        order.order.amountOut = Uint::from(100000000);
        let result = calculate_reward(order, amm);
        assert!(result.is_ok());
    }

    #[test]
    fn handles_ask_order() {
        let mut rng = thread_rng();
        let amm = generate_amm_market(100000);
        let mut order = generate_top_of_block_order(&mut rng, true, None, None);
        order.is_bid = false;
        order.order.amountOut = Uint::from(10_000_000_000_000_u128);
        order.order.amountIn = Uint::from(800000000);
        let result = calculate_reward(order, amm);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn local_test_of_mock() {
        let provider = ProviderBuilder::new()
            .with_recommended_fillers()
            .on_http("http://localhost:8545".parse().unwrap());

        let mock_tob_addr = address!("4026bA349706b18b9dA081233cc20B3C5B4bE980");
        let mock_tob = MockRewardsManagerInstance::new(mock_tob_addr, &provider);
        // These are TEMPROARY LOCAL ADDRESSES from Dave's Testnet - if you are seeing
        // these used in prod code they are No Bueno
        let asset1 = address!("76ca03a67C049477FfB09694dFeF00416dB69746");
        let asset0 = address!("1696C7203769A71c97Ca725d42b13270ee493526");

        // Build a ToB outcome that we care about
        let mut rng = thread_rng();
        let amm = generate_amm_market(100000);
        let mut order = generate_top_of_block_order(&mut rng, true, None, None);
        let total_payment = Uint::from(10_000_000_000_000_u128);
        order.order.amountIn = total_payment;
        order.order.amountOut = Uint::from(100000000);
        let tob_outcome = calculate_reward(order, amm).expect("Error calculating tick donations");
        println!("Outcome: {:?}", tob_outcome);
        // ---- Manually do to_donate to be in our new structs
        let mut donations = tob_outcome.tick_donations.iter().collect::<Vec<_>>();
        // Will sort from lowest to highest (donations[0] will be the lowest tick
        // number)
        donations.sort_by_key(|f| f.0);
        // Each reward value is the cumulative sum of the rewards before it
        let quantities = donations
            .iter()
            .scan(U256::ZERO, |state, (_tick, q)| {
                *state += **q;
                Some(u128::try_from(*state).unwrap())
            })
            .collect::<Vec<_>>();
        let update = RewardsUpdate {
            startTick: *donations[0].0 + 1,
            startLiquidity: tob_outcome.start_liquidity,
            quantities
        };
        let update = PoolRewardsUpdate { asset0: 0, asset1: 1, update };
        println!("PoolRewardsUpdate: {:?}", update);
        // ---- End of all that

        let address_list = [asset0, asset1]
            .into_iter()
            .map(|addr| Asset { addr, borrow: 0, save: 0, settle: 0 })
            .collect();
        let tob_mock_message = MockContractMessage { addressList: address_list, update };
        let tob_bytes = Bytes::from(pade::PadeEncode::pade_encode(&tob_mock_message));
        let call = mock_tob.reward(tob_bytes);
        let call_return = call.call().await;
        let logs = provider.get_logs(&Filter::new()).await.unwrap();
        println!("Logs: {:?}", logs);
        assert!(call_return.is_ok(), "Failed to perform reward call!");
        panic!("Butts");
    }

    #[tokio::test]
    async fn deploys_uniswap_contract() {
        // Start up our Anvil instance
        let provider = ProviderBuilder::new()
            .with_recommended_fillers()
            .on_anvil_with_wallet();
        // Deploy the supporting contracts
        let pool_manager = PoolManager::deploy(&provider, U256::from(50_000_u32))
            .await
            .unwrap();

        // let pool_gate = PoolGate::deploy(&provider, *pool_manager.address())
        //     .await
        //     .unwrap();
        let mock_tob_addr = testing_tools::contracts::deploy_mock_rewards_manager(
            &provider,
            *pool_manager.address()
        )
        .await;
        let mock_tob = MockRewardsManagerInstance::new(mock_tob_addr, &provider);

        // These are TEMPROARY LOCAL ADDRESSES from Dave's Testnet - if you are seeing
        // these used in prod code they are No Bueno
        let asset1 = address!("76ca03a67C049477FfB09694dFeF00416dB69746");
        let asset0 = address!("1696C7203769A71c97Ca725d42b13270ee493526");

        // Build a ToB outcome that we care about
        let mut rng = thread_rng();
        let amm = generate_amm_market(100000);
        let mut order = generate_top_of_block_order(&mut rng, true, None, None);
        let total_payment = Uint::from(10_000_000_000_000_u128);
        order.order.amountIn = total_payment;
        order.order.amountOut = Uint::from(100000000);
        let tob_outcome = calculate_reward(order, amm).expect("Error calculating tick donations");
        println!("Outcome: {:?}", tob_outcome);
        // ---- Manually do to_donate to be in our new structs
        let mut donations = tob_outcome.tick_donations.iter().collect::<Vec<_>>();
        // Will sort from lowest to highest (donations[0] will be the lowest tick
        // number)
        donations.sort_by_key(|f| f.0);
        // Each reward value is the cumulative sum of the rewards before it
        let quantities = donations
            .iter()
            .scan(U256::ZERO, |state, (_tick, q)| {
                *state += **q;
                Some(u128::try_from(*state).unwrap())
            })
            .collect::<Vec<_>>();
        let update = RewardsUpdate {
            startTick: *donations[0].0 + 1,
            startLiquidity: tob_outcome.start_liquidity,
            quantities
        };
        let update = PoolRewardsUpdate { asset0: 0, asset1: 1, update };
        println!("PoolRewardsUpdate: {:?}", update);
        // ---- End of all that

        let address_list = [asset0, asset1]
            .into_iter()
            .map(|addr| Asset { addr, borrow: 0, save: 0, settle: 0 })
            .collect();
        let tob_mock_message = MockContractMessage { addressList: address_list, update };
        let tob_bytes = Bytes::from(pade::PadeEncode::pade_encode(&tob_mock_message));
        let call = mock_tob.reward(tob_bytes);
        let call_return = call.call().await;
        let logs = provider.get_logs(&Filter::new()).await.unwrap();
        println!("Logs: {:?}", logs);
        assert!(call_return.is_ok(), "Failed to perform reward call!");
        panic!("Butts");
    }
}
