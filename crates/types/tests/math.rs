use alloy::primitives::{I256, U256};
use uniswap_v3_math::sqrt_price_math::get_next_sqrt_price_from_amount_0_rounding_up;

#[test]
fn compute_swap_step_oddity() {
    let sqrt_ratio_current_x_96 =
        U256::from_str_radix("11767323680736749590750333513936", 10).unwrap();
    let sqrt_ratio_target_x_96 =
        U256::from_str_radix("11732076369431848475717225902382", 10).unwrap();
    // let liquidity = 340282366920938463463374607429786615964_u128;
    let liquidity = 1_747_735_933_952_538_463_463_374_607_429_786_u128;
    let _amount_u256 = U256::from(40816066_u128);
    // let amount_u256 = U256::from(100_u128);
    let amount_remaining = I256::unchecked_from(1_u128);
    let fee_pips = 0;
    let res = uniswap_v3_math::swap_math::compute_swap_step(
        sqrt_ratio_current_x_96,
        sqrt_ratio_target_x_96,
        liquidity,
        amount_remaining,
        fee_pips
    )
    .unwrap();

    println!("Res: {res:?}");
    let next_res = get_next_sqrt_price_from_amount_0_rounding_up(
        sqrt_ratio_current_x_96,
        liquidity,
        res.1,
        true
    )
    .unwrap();
    println!("Next Res: {next_res:?}");
}
