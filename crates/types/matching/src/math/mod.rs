use std::ops::Neg;

use malachite::{
    Integer, Natural, Rational,
    num::{
        arithmetic::traits::{DivRound, FloorSqrt, Pow, PowerOf2, SaturatingSub},
        basic::traits::{One, Two, Zero},
        conversion::traits::{RoundingFrom, SaturatingInto}
    },
    rounding_modes::RoundingMode
};
use tracing::debug;

use super::{Ray, SqrtPriceX96, const_1e6, const_1e27, uniswap::Direction};

/// Given an AMM with a constant liquidity, a debt, and a quantity of T0 will
/// find the amount of T0 to feed into both the AMM and the debt to ensure that
/// their price winds up at an equal point
#[allow(unused)]
pub fn equal_move_solve() -> Integer {
    Integer::default()
}

/// For a given quantity, computes Q/(1 - F) which is primarily used to figure
/// out how much T0 was input pre-fee given a post-fee quantity of T0 for a bid
/// order
pub fn add_t0_bid_fee(quantity: u128, fee: u128) -> u128 {
    let fee_factor = const_1e6().saturating_sub(Natural::from(fee));
    let numerator = Natural::from(quantity) * const_1e6();
    let res = numerator.div_round(fee_factor, RoundingMode::Ceiling).0;
    (&res).saturating_into()
}

/// For a given quantity, computes Q * (1 - F) which is primarily used to figure
/// out the post-fee quantity of T0 available to the market for an ask order
pub fn sub_t0_ask_fee(quantity: u128, fee: u128) -> u128 {
    let fee_factor = const_1e6().saturating_sub(Natural::from(fee));
    let numerator = Natural::from(quantity) * const_1e6();
    let res = numerator.div_round(fee_factor, RoundingMode::Ceiling).0;
    (&res).saturating_into()
}

/// Output should be:
/// (t1 liquidity change, t0 allocated to order fulfillment post-gas, t0
/// allocated to fee)
pub fn get_quantities_at_price(
    is_bid: bool,
    exact_in: bool,
    fill_amount: u128,
    gas: u128,
    fee: u128,
    ray_ucp: Ray // this is t1 / t0
) -> (u128, u128, u128) {
    // Recreate our calculation that we do in the contract to make sure the numbers
    // all check out

    // tracing::trace!(is_bid, exact_in, fill_amount, gas, fee, ?ray_ucp, "Detail
    // price/quantity");

    match (is_bid, exact_in) {
        // ExactIn Bid - fill_amount is the exact amount of T1 being input to get a T0 output
        (true, true) => {
            // Invert our price because we're working with a bid
            let bid_price = ray_ucp.inv_ray();
            // Find the fee price
            let bid_fee_price = bid_price.scale_to_fee(fee);

            // price = t0 /t1

            // The total amount of t0 exchanged at UCP for the input T1
            let exchanged_t0 = bid_price.quantity(fill_amount, false);
            // The amount of T0 post-fee that will be the output of this order (gas is
            // subtracted from this)
            let net_t0 = bid_fee_price.quantity(fill_amount, false);
            let contract_t0 = exchanged_t0.saturating_sub(net_t0);
            (fill_amount, net_t0.saturating_sub(gas), contract_t0)
        }
        // ExactOut Bid - fill amount is the exact amount of T0 being output for a T1 input
        (true, false) => {
            // Invert our price because we're working with a bid
            let bid_price = ray_ucp.inv_ray();
            // Find the fee price
            let bid_fee_price = bid_price.scale_to_fee(fee);

            // Find out how much T1 it would take to purchase the ExactOut quantity and gas
            // post-fee
            let t1_required = bid_fee_price.inverse_quantity(fill_amount + gas, true);
            // Find out how much that T0 that T1 could have purchased at the base UCP
            let total_t0_purchased = bid_price.quantity(t1_required, false);
            // Use the difference between the two to calculate the fee in T0
            let contract_t0 = total_t0_purchased.saturating_sub(fill_amount + gas);
            (t1_required, fill_amount, contract_t0)
        }
        // ExactIn Ask - fill amount is the exact amount of T0 being input for a T1 output
        (false, true) => {
            // Get our adjusted fee price
            let ask_fee_price = ray_ucp.scale_to_fee(fee);

            // Find the amount of T1 we will return to the user
            let net_t1_out = ask_fee_price.quantity(fill_amount.saturating_sub(gas), false);

            // The amount of T0 you could buy with that T1 at UCP is the net T0 sold
            let net_t0_sold = ray_ucp.inverse_quantity(net_t1_out, true);

            // The contract fee is TotalT0 - Gas - NetT0Sold
            let contract_t0 = fill_amount.saturating_sub(gas).saturating_sub(net_t0_sold);

            (net_t1_out, net_t0_sold, contract_t0)
        }
        // ExactOut Ask - fill amount is the exact amount of T1 expected out for a given T0
        // input
        (false, false) => {
            // Get our adjusted fee price
            let ask_fee_price = ray_ucp.scale_to_fee(fee);

            // fill_amount is the exact amount of T1 we want to move

            // This is the total t0 input - net + fee + gas
            let total_t0_input = ask_fee_price.inverse_quantity(fill_amount, true) + gas;

            // How much T0 would it have cost to fill this T1 at the more advantageous price
            // (net only)
            let net_t0 = ray_ucp.inverse_quantity(fill_amount, true);

            let contract_fee = total_t0_input.saturating_sub(gas).saturating_sub(net_t0);

            (fill_amount, net_t0, contract_fee)
        }
    }
}

/// Given a quantity of input T0 as well as an AMM with constant liquidity and a
/// debt that are at the same initial price, this will find the amount of T0 out
/// of the total input amount that should be given to the AMM and the debt in
/// order to ensure that they both end up at the closest price possible.
pub fn amm_debt_same_move_solve(
    amm_liquidity: u128,
    debt_initial_t0: u128,
    debt_fixed_t1: u128,
    quantity_moved: u128,
    direction: Direction
) -> u128 {
    let l = Integer::from(amm_liquidity);
    let l_squared = (&l).pow(2);

    // The precision we want to use for this operation, depending on our common
    // values we might need to adjust this
    let precision: usize = 192;

    // a = T1d / L^2
    let a_frac =
        Rational::from_integers_ref(&(Integer::from(debt_fixed_t1) << precision), &l_squared);
    let a = Integer::rounding_from(a_frac, RoundingMode::Nearest).0;

    debug!(a = ?a, "A factor");

    // b = 2(sqrt(t0Debt * t1Debt)/L) + 1
    let dt0 = Integer::from(debt_initial_t0) << precision;
    let dt1 = Integer::from(debt_fixed_t1) << precision;
    let mul_debt = dt0 * dt1;
    let sqrt_debt = mul_debt.floor_sqrt();
    let debt_numerator = (Integer::TWO * sqrt_debt) + (l.clone() << precision);
    let b = debt_numerator.div_round(l, RoundingMode::Nearest).0;
    // let debt_numerator = ((Integer::from(debt_initial_t0) *
    // Integer::from(debt_fixed_t1))     << (precision * 2))
    //     .floor_sqrt()
    //     * Integer::TWO;

    // let b = debt_numerator.div_round(l, RoundingMode::Nearest).0 + (Integer::ONE
    // << precision);

    debug!(b = ?b, "B factor");

    // c = -T
    let c = match direction {
        // If the market is selling T0 to us, then we are on the bid side.  T is negative so -T is
        // positive
        Direction::SellingT0 => Integer::from(quantity_moved),
        // If the market is buying T0 from us, then we are on the ask side.  T is positive so -T is
        // negative
        Direction::BuyingT0 => Integer::from(quantity_moved).neg()
    } << precision;

    debug!(c = ?c, "C factor");

    let solution = quadratic_solve(a, b, c, precision);
    println!("Got solutions: {solution:?}");
    solution
        .0
        .filter(|i| match direction {
            Direction::BuyingT0 => *i <= Integer::ZERO,
            Direction::SellingT0 => *i >= Integer::ZERO
        })
        .map(|i| resolve_precision(precision, i, RoundingMode::Ceiling))
        .filter(|i| *i < quantity_moved)
        .or(solution
            .1
            .filter(|i| match direction {
                Direction::BuyingT0 => *i <= Integer::ZERO,
                Direction::SellingT0 => *i >= Integer::ZERO
            })
            .map(|i| resolve_precision(precision, i, RoundingMode::Ceiling))
            .filter(|i| *i < quantity_moved))
        // If nothing else works, we can presume it all goes to the AMM?
        .unwrap_or(quantity_moved)
}

/// Given an AMM with a constant liquidity and a debt, this will find the
/// quantity of T0 you can buy from the AMM and feed into the debt such that
/// their prices end up as close as possible
pub fn price_intersect_solve(
    amm_liquidity: u128,
    amm_price: SqrtPriceX96,
    debt_fixed_t1: u128,
    debt_price: Ray,
    direction: Direction
) -> Integer {
    debug!(amm_liquidity, amm_price = ?amm_price, debt_t1 = debt_fixed_t1, debt_price = ?debt_price, "Price intersect solve");
    let l = Integer::from(amm_liquidity);
    let l_squared = (&l).pow(2);
    let amm_sqrt_price_x96 = Integer::from(Natural::from_limbs_asc(amm_price.as_limbs()));
    let debt_magnitude = Integer::from(debt_fixed_t1);

    // The precision we want to use for this operation, depending on our common
    // values we might need to adjust this
    let precision: usize = 192;

    // a = 1/L^2
    let a_frac = Rational::from_integers_ref(&(Integer::ONE << precision), &l_squared);
    let a = Integer::rounding_from(a_frac, RoundingMode::Nearest).0;
    debug!(a = ?a, "A factor");

    // b = [ 2/(L*sqrt(Pa)) - 1/(T1d) ]
    let b_first_part = Rational::from_integers_ref(
        &(Integer::TWO << (96 + precision)),
        &(&l * &amm_sqrt_price_x96)
    );
    let b_second_part = Rational::from_integers_ref(&(Integer::ONE << precision), &debt_magnitude);
    let b = Integer::rounding_from(b_first_part - b_second_part, RoundingMode::Nearest).0;
    debug!(b = ?b, "B factor");

    // c = [ 1/Pa - 1/Pd ]
    // Precision is x96
    let c_part_1 = Rational::from_integers(
        Integer::ONE << (192 + precision),
        Integer::from(Natural::from_limbs_asc(amm_price.as_price_x192().as_limbs()))
    );
    // Precision is x96
    let c_part_2 = Rational::from_integers(
        (Integer::ONE * Integer::from(const_1e27())) << precision,
        Integer::from(Natural::from_limbs_asc(debt_price.as_limbs()))
    );
    let c = Integer::rounding_from(c_part_1 - c_part_2, RoundingMode::Nearest).0;
    debug!(c = ?c, "C factor");

    let solution = quadratic_solve(a, b, c, precision);
    // Use the direction to find the best possible solution
    // If we're BuyingT0 we want to find the smallest negative number possible
    // If we're SellingT0 we want to find the smallest positive number possible
    match (direction, solution) {
        (_, (None, None)) => panic!("No valid answer to a quadratic solve"),
        // If there's only one valid answer, we just return that
        (_, (Some(a), None)) | (_, (None, Some(a))) => a,
        (Direction::BuyingT0, (Some(a), Some(b))) => {
            if a <= Integer::ZERO {
                if b <= Integer::ZERO {
                    // They're both negative, we want the number with the lowest magnitude
                    std::cmp::max(a, b)
                } else {
                    a
                }
            } else {
                b
            }
        }
        (Direction::SellingT0, (Some(a), Some(b))) => {
            if a >= Integer::ZERO {
                if b >= Integer::ZERO {
                    // They're both positive, we want the number with the lowest magnitude
                    std::cmp::min(a, b)
                } else {
                    a
                }
            } else {
                b
            }
        }
    }
}

pub fn quadratic_solve(
    a: Integer,
    b: Integer,
    c: Integer,
    precision: usize
) -> (Option<Integer>, Option<Integer>) {
    let numerator = (&c * &Integer::TWO) << precision;
    let b_squared = b.clone().pow(2);
    let four_a_c = Integer::from(4_u128) * a * c;
    let sqrt_b24ac = Integer::floor_sqrt(&b_squared - &four_a_c);
    let neg_b = b.neg();

    // Find our denominators for both the + and - solution
    let denom_minus = &neg_b - &sqrt_b24ac;
    let denom_plus = &neg_b + &sqrt_b24ac;
    debug!(numerator = ?numerator, denom_plus = ?denom_plus, denom_minus = ?denom_minus, "Quadratic solve factors");

    // Save ourselves from zeroes
    match (denom_plus == Integer::ZERO, denom_minus == Integer::ZERO) {
        (true, true) => panic!("Both denominators in quadratic solve were zero, this math sucks"),
        // Just one that's valid, return that
        (false, true) => (None, Some(numerator.div_round(&denom_plus, RoundingMode::Nearest).0)),
        // Just one that's valid, return that
        (true, false) => (Some(numerator.div_round(&denom_minus, RoundingMode::Nearest).0), None),
        // Both valid, compare and return the best option
        (false, false) => {
            let answer_plus = numerator
                .clone()
                .div_round(&denom_plus, RoundingMode::Nearest)
                .0;
            let answer_minus = numerator.div_round(&denom_minus, RoundingMode::Nearest).0;
            (Some(answer_minus), Some(answer_plus))
        }
    }
}

pub fn resolve_precision(precision: usize, number: Integer, rm: RoundingMode) -> u128 {
    number
        .div_round(Integer::power_of_2(precision as u64), rm)
        .0
        .unsigned_abs_ref()
        .saturating_into()
}

/// Take two items that can be compared and return them as a tuple with the
/// "lower" item as the first element and the higher item as the second element
pub fn low_to_high<'a, T: Ord>(a: &'a T, b: &'a T) -> (&'a T, &'a T) {
    match a.cmp(b) {
        std::cmp::Ordering::Greater => (b, a),
        _ => (a, b)
    }
}

pub fn max_t1_for_t0(t0: u128, direction: Direction, price: Ray) -> u128 {
    match direction {
        // If we're buying we always round down so it's the amount it'd take to buy (t0 + 1) - 1
        Direction::BuyingT0 => price.quantity(t0 + 1, true).saturating_sub(1),
        // If we're selling, we always round up, so the max for a quantity is just what's at the
        // quantity
        Direction::SellingT0 => price.quantity(t0, false)
    }
}

#[cfg(test)]
mod tests {
    use alloy_primitives::{I256, U256};
    use uniswap_v3_math::{swap_math::compute_swap_step, tick_math::MAX_TICK};

    use super::*;
    use crate::matching::SqrtPriceX96;

    #[test]
    fn quadratic_solve_test() {
        let amm_liquidity = 1_000_000_000_000_000_u128;
        let amm_price = SqrtPriceX96::at_tick(150000).unwrap();
        let debt_price = Ray::from(SqrtPriceX96::at_tick(110000).unwrap());
        let debt_start_t0 = 1_000_000_000_u128;
        let debt_fixed_t1: u128 = debt_price.mul_quantity(U256::from(debt_start_t0)).to();
        let res = price_intersect_solve(
            amm_liquidity,
            amm_price,
            debt_fixed_t1,
            debt_price,
            Direction::BuyingT0
        );
        debug!(result = ?res, "Solution");
        // RoundingMode has to be UP here we want the greater value at all times
        let quantity = resolve_precision(192, res, RoundingMode::Up);
        debug!(quantity, "Quantity found");

        // Validate that the quantity returned brings the two prices as close together
        // as possible.  We do this by checking the result against result+1 and result-1
        let max_tick_target = SqrtPriceX96::at_tick(MAX_TICK).unwrap();
        let price_gaps = [Some(quantity), Some(quantity + 1), quantity.checked_sub(1)].map(|e| {
            e.map(|q| {
                let amount_remaining = I256::unchecked_from(q) * I256::MINUS_ONE;
                let amm_result = compute_swap_step(
                    amm_price.into(),
                    max_tick_target.into(),
                    amm_liquidity,
                    amount_remaining,
                    0
                )
                .unwrap();
                let amm_final_price = Ray::from(SqrtPriceX96::from(amm_result.0));
                let debt_final_price =
                    Ray::calc_price(U256::from(debt_start_t0 - q), U256::from(debt_fixed_t1));
                amm_final_price.abs_diff(*debt_final_price)
            })
        });
        let closer_than_plus_one = price_gaps[0].unwrap() < price_gaps[1].unwrap();
        let closer_than_minus_one = price_gaps[2]
            .map(|r| price_gaps[0].unwrap() < r)
            .unwrap_or(true);
        assert!(
            closer_than_plus_one && closer_than_minus_one,
            "Quantity found was not minimum price gap!"
        );
    }

    #[test]
    fn debt_same_move_solve_test() {
        let amm_price = Ray::from(SqrtPriceX96::at_tick(100000).unwrap());
        let debt_fixed_t1 = 10_000_000_000_u128;
        let debt_initial_t0 = amm_price.inverse_quantity(debt_fixed_t1, true);
        println!("Debt initial T0: {debt_initial_t0}");
        let amm_liquidity = 1_000_000_000_000_000_u128;
        let total_input = 1_000_000_000_u128;
        let amm_portion = amm_debt_same_move_solve(
            amm_liquidity,
            debt_initial_t0,
            debt_fixed_t1,
            total_input,
            Direction::SellingT0
        );
        println!("AMM portion: {amm_portion}");
        let debt_portion = total_input - amm_portion;
        println!("Debt portion: {debt_portion}");
    }

    #[test]
    fn get_amount_at_price_test() {
        let ray_ucp = Ray::from(SqrtPriceX96::at_tick(100000).unwrap());
        let res = get_quantities_at_price(
            true,
            true,
            1_000_000_000_000_000_000_u128,
            10,
            100000_u128,
            ray_ucp
        );

        assert_eq!(
            res.0, 1_000_000_000_000_000_000_u128,
            "Input quantity not aligned for ExactIn Bid"
        );

        assert_eq!(
            res.1 + res.2 + 10,
            ray_ucp.inverse_quantity(res.0, false),
            "Unbalanced numbers in ExactIn Bid"
        );

        let res = get_quantities_at_price(
            true,
            false,
            1_000_000_000_000_000_000_u128,
            10,
            100000_u128,
            ray_ucp
        );

        assert_eq!(
            res.1, 1_000_000_000_000_000_000_u128,
            "Didn't receive right quantity of T0 for ExactOut Bid"
        );

        assert_eq!(
            res.1 + res.2 + 10,
            ray_ucp.inverse_quantity(res.0, false),
            "Unbalanced numbers in ExactOut Bid"
        );

        let res = get_quantities_at_price(
            false,
            true,
            1_000_000_000_000_000_000_u128,
            10,
            100000_u128,
            ray_ucp
        );

        // Gas + Fee + Net == input quantity
        assert_eq!(
            res.1 + res.2 + 10,
            1_000_000_000_000_000_000_u128,
            "T0 outputs do not equal input for ExactIn Ask"
        );
        // Validate that you sold the rest at UCP
        assert_eq!(
            res.1,
            ray_ucp.inverse_quantity(res.0, true),
            "Unbalanced numbers in ExactIn Ask"
        );

        let res = get_quantities_at_price(
            false,
            false,
            1_000_000_000_000_000_000_u128,
            10,
            100000_u128,
            ray_ucp
        );

        assert_eq!(
            res.0, 1_000_000_000_000_000_000_u128,
            "Output numbers incorrect for ExactOut Ask"
        );

        // Gas + Fee + Net == input quantity
        assert_eq!(
            res.1,
            ray_ucp.inverse_quantity(res.0, true),
            "Unbalanced numbers in ExactOut Ask"
        );
    }
}
