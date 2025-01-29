use std::ops::Neg;

use malachite::{
    num::{
        arithmetic::traits::{DivRound, FloorSqrt, Pow, PowerOf2},
        basic::traits::{One, Two, Zero},
        conversion::traits::{RoundingFrom, SaturatingInto}
    },
    rounding_modes::RoundingMode,
    Integer, Natural, Rational
};
use tracing::debug;

use super::{const_1e27, uniswap::Direction, Ray, SqrtPriceX96};

/// Given an AMM with a constant liquidity, a debt, and a quantity of T0 will
/// find the amount of T0 to feed into both the AMM and the debt to ensure that
/// their price winds up at an equal point
#[allow(unused)]
pub fn equal_move_solve() -> Integer {
    Integer::default()
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
    println!("Got solutions: {:?}", solution);
    let answer = solution
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
        .unwrap_or(quantity_moved);
    answer
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
    solution.0.or(solution.1).unwrap()
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
        Direction::BuyingT0 => price.quantity(t0, true).saturating_sub(1),
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

    // Helper function to create test prices
    fn create_test_price(tick: i32) -> Ray {
        Ray::from(SqrtPriceX96::at_tick(tick).unwrap())
    }

    #[test]
    fn test_equal_move_solve() {
        // Currently returns default, so just test that
        assert_eq!(equal_move_solve(), Integer::default());
    }

    #[test]
    fn test_amm_debt_same_move_solve() {
        // Test case 1: Standard selling T0 direction
        let result = amm_debt_same_move_solve(
            1_000_000_000_000_000_u128, // amm_liquidity
            1_000_000_000_u128,         // debt_initial_t0
            10_000_000_000_u128,        // debt_fixed_t1
            1_000_000_000_u128,         // quantity_moved
            Direction::SellingT0
        );
        assert!(result <= 1_000_000_000_u128);

        // Test case 2: Standard buying T0 direction
        let result_buy = amm_debt_same_move_solve(
            1_000_000_000_000_000_u128,
            1_000_000_000_u128,
            10_000_000_000_u128,
            1_000_000_000_u128,
            Direction::BuyingT0
        );
        assert!(result_buy <= 1_000_000_000_u128);

        // Test case 3: Edge case with very small liquidity
        let small_result = amm_debt_same_move_solve(
            1_u128, // minimal liquidity
            1_000_000_000_u128,
            10_000_000_000_u128,
            1_000_000_000_u128,
            Direction::SellingT0
        );
        assert!(small_result <= 1_000_000_000_u128);

        // Test case 4: Edge case with very large liquidity
        let large_result = amm_debt_same_move_solve(
            u128::MAX, // maximum liquidity
            1_000_000_000_u128,
            10_000_000_000_u128,
            1_000_000_000_u128,
            Direction::SellingT0
        );
        assert!(large_result <= 1_000_000_000_u128);

        // Test case 5: Zero quantity moved
        let zero_result = amm_debt_same_move_solve(
            1_000_000_000_000_000_u128,
            1_000_000_000_u128,
            10_000_000_000_u128,
            0,
            Direction::SellingT0
        );
        assert_eq!(zero_result, 0);
    }

    #[test]
    fn test_price_intersect_solve() {
        // Test case 1: Standard case with different prices
        let amm_liquidity = 1_000_000_000_000_000_u128;
        let amm_price = SqrtPriceX96::at_tick(150000).unwrap();
        let debt_price = Ray::from(SqrtPriceX96::at_tick(110000).unwrap());
        let debt_fixed_t1 = 10_000_000_000_u128;

        let result = price_intersect_solve(
            amm_liquidity,
            amm_price,
            debt_fixed_t1,
            debt_price,
            Direction::BuyingT0
        );
        assert!(result != Integer::ZERO);

        // Test case 2: Very close prices
        let close_price = SqrtPriceX96::at_tick(150001).unwrap();
        let result_close = price_intersect_solve(
            amm_liquidity,
            amm_price,
            debt_fixed_t1,
            Ray::from(close_price),
            Direction::BuyingT0
        );
        assert!(result_close != Integer::ZERO);

        // Test case 3: Extreme price difference
        let extreme_price = SqrtPriceX96::at_tick(-150000).unwrap();
        let result_extreme = price_intersect_solve(
            amm_liquidity,
            amm_price,
            debt_fixed_t1,
            Ray::from(extreme_price),
            Direction::SellingT0
        );
        assert!(result_extreme != Integer::ZERO);

        // Test case 4: Minimal liquidity
        let result_min_liq = price_intersect_solve(
            1_u128,
            amm_price,
            debt_fixed_t1,
            debt_price,
            Direction::BuyingT0
        );
        assert!(result_min_liq != Integer::ZERO);

        // Test case 5: Maximum liquidity
        let result_max_liq = price_intersect_solve(
            u128::MAX,
            amm_price,
            debt_fixed_t1,
            debt_price,
            Direction::BuyingT0
        );
        assert!(result_max_liq != Integer::ZERO);
    }

    #[test]
    fn test_quadratic_solve() {
        // Test case 1: Standard case with two real solutions
        let (sol1, sol2) = quadratic_solve(
            Integer::from(1), // x² term
            Integer::from(5), // x term
            Integer::from(6), // constant term
            64                // precision
        );
        assert!(sol1.is_some() && sol2.is_some());
        if let (Some(s1), Some(s2)) = (&sol1, &sol2) {
            assert!(s1 != s2, "Solutions should be different");
        }

        // Test case 2: One real solution (discriminant = 0)
        let (sol1, sol2) =
            quadratic_solve(Integer::from(1), Integer::from(2), Integer::from(1), 64);
        assert!(sol1.is_some() || sol2.is_some());

        // Test case 3: Large coefficients
        let (sol1, sol2) = quadratic_solve(
            Integer::from(1000000),
            Integer::from(2000000),
            Integer::from(1000000),
            64
        );
        assert!(sol1.is_some() || sol2.is_some());

        // Test case 4: Negative coefficients
        let (sol1, sol2) =
            quadratic_solve(Integer::from(-1), Integer::from(-5), Integer::from(-6), 64);
        assert!(sol1.is_some() || sol2.is_some());

        // Test case 5: Zero a coefficient (linear equation)
        let (sol1, sol2) = quadratic_solve(Integer::ZERO, Integer::from(2), Integer::from(1), 64);
        assert!(sol1.is_some() || sol2.is_some());
    }

    #[test]
    #[should_panic(expected = "Both denominators in quadratic solve were zero")]
    fn test_quadratic_solve_both_zero() {
        quadratic_solve(
            Integer::ZERO, // a
            Integer::ZERO, // b
            Integer::ZERO, // c
            64             // precision
        );
    }

    #[test]
    fn test_resolve_precision() {
        let number = Integer::from(1000) << 64;
        let result = resolve_precision(64, number, RoundingMode::Nearest);
        assert_eq!(result, 1000);

        // Test rounding modes
        let number: Integer = (Integer::from(1500) << 64) + (Integer::from(1) << 63);
        let ceiling = resolve_precision(64, number.clone(), RoundingMode::Ceiling);
        let floor = resolve_precision(64, number, RoundingMode::Floor);
        assert!(ceiling > floor);
    }

    #[test]
    fn test_low_to_high() {
        let a = 5;
        let b = 10;

        let (low, high) = low_to_high(&a, &b);
        assert_eq!(*low, 5);
        assert_eq!(*high, 10);

        let (low, high) = low_to_high(&b, &a);
        assert_eq!(*low, 5);
        assert_eq!(*high, 10);

        let (low, high) = low_to_high(&a, &a);
        assert_eq!(*low, 5);
        assert_eq!(*high, 5);
    }

    #[test]
    fn test_max_t1_for_t0() {
        // Setup different price levels for testing
        let low_price = create_test_price(-50_000);
        let mid_price = create_test_price(100_000);
        let high_price = create_test_price(150_000);

        // // Test case 1: Standard amounts at different price levels
        for price in [low_price, mid_price, high_price] {
            let max_buy = max_t1_for_t0(1000, Direction::BuyingT0, price);
            let max_sell = max_t1_for_t0(1000, Direction::SellingT0, price);
            assert!(max_buy > 0);
            assert!(max_sell > 0);
            assert!(max_sell >= max_buy, "sell {:?} buy: {:?}", max_sell, max_buy);
        }

        // Test case 2: Edge cases
        for direction in [Direction::BuyingT0, Direction::SellingT0] {
            // Zero input
            assert_eq!(max_t1_for_t0(0, direction, mid_price), 0);
            // Very small input
            assert!(max_t1_for_t0(1, direction, mid_price) > 0);
            // Very large input
            let large_result = max_t1_for_t0(u128::MAX - 1, direction, mid_price);
            assert!(large_result > 0);
        }

        // Test case 3: Price relationship verification
        let test_amount = 1000_u128;
        for direction in [Direction::BuyingT0, Direction::SellingT0] {
            let low_result = max_t1_for_t0(test_amount, direction, low_price);
            let mid_result = max_t1_for_t0(test_amount, direction, mid_price);
            let high_result = max_t1_for_t0(test_amount, direction, high_price);

            // Higher prices should result in more T1 for same T0
            assert!(high_result >= mid_result);
            assert!(mid_result >= low_result);
        }

        // Test case 4: Consecutive values
        let test_price = mid_price;
        for i in 1..10 {
            let current = max_t1_for_t0(i, Direction::SellingT0, test_price);
            let next = max_t1_for_t0(i + 1, Direction::SellingT0, test_price);
            assert!(next > current, "T1 should increase with T0");
        }
    }

    #[test]
    fn debt_same_move_solve_test() {
        let amm_price = Ray::from(SqrtPriceX96::at_tick(100000).unwrap());
        let debt_fixed_t1 = 10_000_000_000_u128;
        let debt_initial_t0 = amm_price.inverse_quantity(debt_fixed_t1, true);
        println!("Debt initial T0: {}", debt_initial_t0);
        let amm_liquidity = 1_000_000_000_000_000_u128;
        let total_input = 1_000_000_000_u128;
        let amm_portion = amm_debt_same_move_solve(
            amm_liquidity,
            debt_initial_t0,
            debt_fixed_t1,
            total_input,
            Direction::SellingT0
        );
        println!("AMM portion: {}", amm_portion);
        let debt_portion = total_input - amm_portion;
        println!("Debt portion: {}", debt_portion);
    }
}
