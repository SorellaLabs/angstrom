use angstrom_types::{
    contract_payloads::tob::ToBOutcome,
    matching::uniswap::PoolSnapshot,
    sol_bindings::{grouped_orders::OrderWithStorageData, rpc_orders::TopOfBlockOrder}
};

// Basically only tests in here now
pub fn calculate_reward(
    tob: &OrderWithStorageData<TopOfBlockOrder>,
    snapshot: &PoolSnapshot
) -> eyre::Result<ToBOutcome> {
    ToBOutcome::from_tob_and_snapshot(tob, snapshot)
}

#[cfg(test)]
mod test {
    use alloy::primitives::Uint;
    use alloy_primitives::U256;
    use angstrom_types::matching::{
        uniswap::{LiqRange, PoolSnapshot},
        SqrtPriceX96
    };
    use angstrom_types::orders::{OrderId, OrderPriorityData};
    use angstrom_types::primitive::PoolId;
    use angstrom_types::sol_bindings::grouped_orders::OrderWithStorageData;
    use angstrom_types::sol_bindings::rpc_orders::TopOfBlockOrder;
    use angstrom_types::sol_bindings::RawPoolOrder;
    use rand::rngs::ThreadRng;
    use rand::{thread_rng, Rng};
    use uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick;

    pub fn generate_top_of_block_order(
        rng: &mut ThreadRng,
        is_bid: bool,
        pool_id: Option<PoolId>,
        valid_block: Option<u64>,
        quantity_in: Option<u128>,
        quantity_out: Option<u128>
    ) -> OrderWithStorageData<TopOfBlockOrder> {
        let pool_id = pool_id.unwrap_or_default();
        let valid_block = valid_block.unwrap_or_default();
        let price: u128 = rng.gen();
        let volume: u128 = rng.gen();
        let gas: U256 = rng.gen();
        let gas_units: u64 = rng.gen();
        let order = TopOfBlockOrder {
            quantity_in: quantity_in.unwrap_or_default(),
            quantity_out: quantity_out.unwrap_or_default(),
            ..TopOfBlockOrder::default()
        };

        let priority_data = OrderPriorityData { price: U256::from(price), volume, gas, gas_units };
        let order_id = OrderId {
            pool_id,
            hash: order.order_hash(),
            ..OrderId::default()
        };
        OrderWithStorageData {
            order,
            priority_data,
            is_bid,
            is_currently_valid: true,
            is_valid: true,
            order_id,
            pool_id,
            valid_block,
            ..OrderWithStorageData::default()
        }
    }

    use super::calculate_reward;

    fn generate_amm_market(target_tick: i32) -> PoolSnapshot {
        let range =
            LiqRange::new(target_tick - 1000, target_tick + 1000, 100_000_000_000_000).unwrap();
        let ranges = vec![range];
        let sqrt_price_x96 = SqrtPriceX96::from(get_sqrt_ratio_at_tick(target_tick).unwrap());
        PoolSnapshot::new(ranges, sqrt_price_x96).unwrap()
    }

    #[test]
    fn calculates_reward() {
        let mut rng = thread_rng();
        let snapshot = generate_amm_market(100000);
        let total_payment = 10_000_000_000_000_u128;
        let tob = generate_top_of_block_order(
            &mut rng,
            true,
            None,
            None,
            Some(total_payment),
            Some(100000000_u128)
        );
        let result = calculate_reward(&tob, &snapshot).expect("Error calculating tick donations");
        let total_donations = result.total_donations();
        assert_eq!(
            total_donations + result.total_cost + result.tribute,
            Uint::from(total_payment),
            "Total allocations do not add up to input payment"
        );
    }

    #[test]
    fn handles_insufficient_funds() {
        let mut rng = thread_rng();
        let snapshot = generate_amm_market(100000);
        let tob = generate_top_of_block_order(
            &mut rng,
            true,
            None,
            None,
            Some(10_000_000_u128),
            Some(100000000_u128)
        );
        let result = calculate_reward(&tob, &snapshot);
        assert!(result.is_err_and(|e| {
            e.to_string()
                .starts_with("Not enough input to cover the transaction")
        }));
    }

    #[test]
    fn handles_precisely_zero_donation() {
        let mut rng = thread_rng();
        let snapshot = generate_amm_market(100000);
        // Hand-calculated that this is the correct payment for this starting price and
        // liquidity
        let total_payment = 2_201_872_310_000_u128;
        let tob = generate_top_of_block_order(
            &mut rng,
            true,
            None,
            None,
            Some(total_payment),
            Some(100000000_u128)
        );
        let result = calculate_reward(&tob, &snapshot).expect("Error calculating tick donations");
        let total_donations = result.total_donations();
        assert!(
            result.tick_donations.is_empty(),
            "Donations are being offered when we shouldn't have any"
        );
        assert_eq!(
            total_donations + result.total_cost + result.tribute,
            Uint::from(total_payment),
            "Total allocations do not add up to input payment"
        );
    }

    #[test]
    fn handles_partial_donation() {
        let mut rng = thread_rng();
        let snapshot = generate_amm_market(100000);
        let partial_donation = 20_000_000_u128;
        let total_payment = 2_201_872_310_000_u128 + partial_donation;
        let tob = generate_top_of_block_order(
            &mut rng,
            true,
            None,
            None,
            Some(total_payment),
            Some(100000000_u128)
        );
        let result = calculate_reward(&tob, &snapshot).expect("Error calculating tick donations");
        let total_donations = result.total_donations();
        assert_eq!(result.tick_donations.len(), 1, "Wrong number of donations");
        assert!(result.tick_donations.contains_key(&99000), "Donation missing");
        assert_eq!(
            result
                .tick_donations
                .get(&99000)
                .unwrap()
                .saturating_to::<u128>(),
            partial_donation,
            "Donation of incorrect size"
        );
        assert_eq!(
            total_donations + result.total_cost + result.tribute,
            Uint::from(total_payment),
            "Total allocations do not add up to input payment"
        );
    }

    #[test]
    fn handles_bid_order() {
        let mut rng = thread_rng();
        let snapshot = generate_amm_market(100000);
        let tob = generate_top_of_block_order(
            &mut rng,
            true,
            None,
            None,
            Some(10_000_000_000_000_u128),
            Some(100000000_u128)
        );
        let result = calculate_reward(&tob, &snapshot);
        assert!(result.is_ok());
    }

    #[test]
    fn handles_ask_order() {
        let mut rng = thread_rng();
        let snapshot = generate_amm_market(100000);
        let tob = generate_top_of_block_order(
            &mut rng,
            false,
            None,
            None,
            Some(10_000_000_000_000_u128),
            Some(800000000_u128)
        );
        let result = calculate_reward(&tob, &snapshot);
        assert!(result.is_ok());
    }

    #[test]
    fn only_rewards_initialized_ticks() {
        let mut rng = thread_rng();
        let snapshot = generate_amm_market(100000);
        let total_payment = 2_203_371_417_593_u128;
        let tob = generate_top_of_block_order(
            &mut rng,
            true,
            None,
            None,
            Some(total_payment),
            Some(100000000_u128)
        );
        let first_tick = 100000 - 1000;
        let result = calculate_reward(&tob, &snapshot).expect("Error calculating tick donations");
        assert!(
            result.tick_donations.len() == 1,
            "Too many donations - only one initialized tick in this market"
        );
        assert!(
            result.tick_donations.contains_key(&first_tick),
            "Donation not made to only initialized tick"
        );
    }
}
