use std::sync::RwLockReadGuard;

use angstrom_types::{
    contract_payloads::tob::ToBOutcome,
    matching::{
        uniswap::{LiqRange, PoolSnapshot},
        SqrtPriceX96
    },
    sol_bindings::{grouped_orders::OrderWithStorageData, rpc_orders::TopOfBlockOrder}
};
use eyre::Error;

use crate::uniswap::{pool::EnhancedUniswapPool, pool_data_loader::PoolDataLoader};
// Basically only tests in here now
pub fn calculate_reward(
    tob: &OrderWithStorageData<TopOfBlockOrder>,
    snapshot: &PoolSnapshot
) -> eyre::Result<ToBOutcome> {
    ToBOutcome::from_tob_and_snapshot(tob, snapshot)
}

pub fn get_market_snapshot<Loader: PoolDataLoader<A>, A>(
    pool_lock: RwLockReadGuard<'_, EnhancedUniswapPool<Loader, A>>
) -> Result<PoolSnapshot, Error> {
    let (ranges, price) = {
        // Grab all ticks with any change in liquidity from our underlying pool data
        let mut tick_vec = pool_lock
            .ticks
            .iter()
            .filter(|tick| tick.1.liquidity_net != 0)
            .collect::<Vec<_>>();
        // Sort the ticks low-to-high
        tick_vec.sort_by_key(|x| x.0);
        // Build our LiqRanges out of our ticks, if any
        let ranges = tick_vec
            .windows(2)
            .map(|tickwindow| {
                let lower_tick = tickwindow[0].0;
                let upper_tick = tickwindow[1].0;
                let liquidity = tickwindow[0].1.liquidity_gross;
                LiqRange::new(*lower_tick, *upper_tick, liquidity)
            })
            .collect::<Result<Vec<_>, _>>()?;
        // Get our starting price
        let price = SqrtPriceX96::from(pool_lock.sqrt_price);
        (ranges, price)
    };
    PoolSnapshot::new(ranges, price)
}

#[cfg(test)]
mod test {
    use alloy::primitives::Uint;
    use angstrom_types::matching::{
        uniswap::{LiqRange, PoolSnapshot},
        SqrtPriceX96
    };
    use rand::thread_rng;
    use testing_tools::type_generator::orders::generate_top_of_block_order;
    use uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick;

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
