use malachite::{
    num::{
        arithmetic::traits::{DivRound, FloorSqrt, Pow},
        basic::traits::{One, Two},
        conversion::traits::{RoundingFrom, SaturatingInto}
    },
    rounding_modes::RoundingMode,
    Integer, Natural, Rational
};

use super::{const_1e27, Ray, SqrtPriceX96};

/// Given an AMM with a constant liquidity, a debt, and a quantity of T0 will
/// find the amount of T0 to feed into both the AMM and the debt to ensure that
/// their price winds up at an equal point
#[allow(unused)]
pub fn equal_move_solve() -> Integer {
    Integer::default()
}

/// Given an AMM with a constant liquidity and a debt, this will find the
/// quantity of T0 you can buy from the AMM and feed into the debt such that
/// their prices end up as close as possible
pub fn price_intersect_solve(
    amm_liquidity: u128,
    amm_price: SqrtPriceX96,
    debt_fixed_t1: u128,
    debt_price: Ray
) -> Integer {
    println!(
        "Doing a price intersect solve:\nAMM Liquidity: {}\nAMM Price: {:?}\nDebt T1: {}\nDebt \
         Price: {:?}",
        amm_liquidity, amm_price, debt_fixed_t1, debt_price
    );
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
    println!("A: {}", a);

    // b = [ 2/(L*sqrt(Pa)) - 1/(T1d) ]
    let b_first_part = Rational::from_integers_ref(
        &(Integer::TWO << (96 + precision)),
        &(&l * &amm_sqrt_price_x96)
    );
    let b_second_part = Rational::from_integers_ref(&(Integer::ONE << precision), &debt_magnitude);
    let b = Integer::rounding_from(b_first_part - b_second_part, RoundingMode::Nearest).0;
    println!("B: {}", b);

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
    println!("C: {}", c);

    quadratic_solve(a, b, c, precision)
}

pub fn quadratic_solve(a: Integer, b: Integer, c: Integer, precision: usize) -> Integer {
    let numerator = (&c * &Integer::TWO) << precision;
    let b_squared = b.clone().pow(2);
    let four_a_c = Integer::from(4_u128) * a * c;
    let denominator = b - Integer::floor_sqrt(b_squared - four_a_c);

    println!("Numerator: {}\nDenominator: {}", numerator, denominator);

    numerator.div_round(&denominator, RoundingMode::Nearest).0
}

pub fn resolve_precision(precision: usize, number: Integer, rm: RoundingMode) -> u128 {
    number
        .div_round(Integer::TWO.pow(precision as u64), rm)
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

#[cfg(test)]
mod tests {
    use alloy_primitives::{I256, U256};
    use malachite::num::{arithmetic::traits::PowerOf2, conversion::traits::SaturatingFrom};
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
        let res = price_intersect_solve(amm_liquidity, amm_price, debt_fixed_t1, debt_price);
        let quantity = u128::saturating_from(
            res.div_round(Integer::power_of_2(192), RoundingMode::Ceiling)
                .0
                .unsigned_abs_ref()
        );

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
}
