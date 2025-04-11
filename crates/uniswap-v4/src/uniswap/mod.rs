use std::cmp::Ordering;

use alloy_primitives::{I256, aliases::I24};
use malachite::num::arithmetic::traits::Sign;
use thiserror::Error;

pub mod loaders;
pub mod pool;
pub mod pool_data_loader;
pub mod pool_factory;
pub mod pool_manager;
pub mod pool_providers;
pub mod tob;

const MIN_I24: i32 = -8_388_608_i32;
const MAX_I24: i32 = 8_388_607_i32;

fn i32_to_i24(val: i32) -> Result<I24, ConversionError> {
    if !(MIN_I24..=MAX_I24).contains(&val) {
        return Err(ConversionError::OverflowErrorI24(val));
    }
    let sign = val.sign();
    let inner = val.abs();

    let mut bytes = [0u8; 3];
    let value_bytes = inner.to_be_bytes();
    bytes[..].copy_from_slice(&value_bytes[1..]);

    let mut new = I24::from_be_bytes(bytes);
    if sign == Ordering::Less {
        new = -new;
    }
    Ok(new)
}

fn i128_to_i256(value: i128) -> I256 {
    let mut bytes = [0u8; I256::BYTES];
    let value_bytes = value.to_be_bytes();
    let signed_byte = if (value_bytes[0] & 0x80) == 0x80 { 0xFF } else { 0x00 };
    bytes[..16].fill(signed_byte);
    bytes[16..].copy_from_slice(&value_bytes);
    I256::from_be_bytes(bytes)
}

#[derive(Error, Debug)]
pub enum ConversionError {
    #[error("overflow from i32 to i24 {0:?}")]
    OverflowErrorI24(i32),
    #[error("overflow from I256 to I128 {0:?}")]
    OverflowErrorI28(I256)
}

#[cfg(test)]
mod tests {
    use alloy_primitives::aliases::I24;

    use crate::uniswap::{MAX_I24, MIN_I24, i32_to_i24};

    #[test]
    #[ignore]
    fn test_i32_to_i24_exhaustive() {
        for original in MIN_I24..=MAX_I24 {
            let converted = i32_to_i24(original).unwrap();
            assert_eq!(
                converted,
                I24::from_dec_str(format!("{}", original).as_str()).unwrap(),
                "i32 to I24 conversion failed"
            );
        }
    }

    #[test]
    fn test_i32_to_i24() {
        let test_values = [
            (I24::MIN).as_i32() + 1,
            -1025_i32,
            1024_i32,
            1000_i32,
            -1000_i32,
            I24::MAX.as_i32() - 1
        ];
        for &original in test_values.iter() {
            let converted = i32_to_i24(original).unwrap();
            assert_eq!(
                converted,
                I24::from_dec_str(format!("{}", original).as_str()).unwrap(),
                "i32 to I24 conversion failed"
            );
        }
        // sanity check consts
        assert_eq!(I24::MIN.to_string(), MIN_I24.to_string());
        assert_eq!(I24::MAX.to_string(), MAX_I24.to_string());
    }

    #[test]
    fn span_sums_and_rounding_work() {
        let liq = 50000000000;
        let t1 = uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick(10).unwrap();
        let t2 = uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick(20).unwrap();
        let t3 = uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick(30).unwrap();

        let step_12 =
            uniswap_v3_math::sqrt_price_math::_get_amount_0_delta(t1, t2, liq, true).unwrap();
        let step_23 =
            uniswap_v3_math::sqrt_price_math::_get_amount_0_delta(t2, t3, liq, true).unwrap();
        let step_13 =
            uniswap_v3_math::sqrt_price_math::_get_amount_0_delta(t1, t3, liq, true).unwrap();

        assert_eq!(step_12 + step_23, step_13, "Sums not equal");
    }

    #[test]
    fn test_ask_iter() {}
}
