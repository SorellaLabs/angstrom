use alloy_primitives::{aliases::I24, I256};
use once_cell::sync::Lazy;
use thiserror::Error;

pub mod pool;
pub mod pool_data_loader;
pub mod pool_manager;
pub mod pool_providers;
pub mod tob;

const MIN_I24: i32 = -8_388_608_i32;
const MAX_I24: i32 = 8_388_607_i32;

static MIN_I128: Lazy<I256> =
    Lazy::new(|| I256::from_dec_str(i128::MIN.to_string().as_str()).unwrap());

static MAX_I128: Lazy<I256> =
    Lazy::new(|| I256::from_dec_str(i128::MAX.to_string().as_str()).unwrap());

fn i32_to_i24(val: i32) -> Result<I24, ConversionError> {
    if !(MIN_I24..=MAX_I24).contains(&val) {
        return Err(ConversionError::OverflowErrorI24(val));
    }
    let mut bytes = [0u8; 3];
    let value_bytes = val.to_be_bytes();
    bytes[..].copy_from_slice(&value_bytes[1..]);
    Ok(I24::from_be_bytes(bytes))
}

fn i128_to_i256(value: i128) -> I256 {
    let mut bytes = [0u8; I256::BYTES];
    let value_bytes = value.to_be_bytes();
    let signed_byte = if (value_bytes[0] & 0x80) == 0x80 { 0xFF } else { 0x00 };
    for byte in &mut bytes[0..16] {
        *byte = signed_byte;
    }
    bytes[16..].copy_from_slice(&value_bytes);
    I256::from_be_bytes(bytes)
}

fn i256_to_i128(value: I256) -> Result<i128, ConversionError> {
    if value < *MIN_I128 || value > *MAX_I128 {
        return Err(ConversionError::OverflowErrorI28(value));
    }
    let value_bytes: [u8; I256::BYTES] = value.to_be_bytes();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&value_bytes[16..]);
    Ok(i128::from_be_bytes(bytes))
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
    use alloy_primitives::{aliases::I24, I256};

    use crate::cfmm::uniswap::{i128_to_i256, i256_to_i128, i32_to_i24, MAX_I24, MIN_I24};

    #[test]
    fn test_i256_to_i128_overflow() {
        let test_values = [
            (
                I256::from_dec_str("-170141183460469231731687303715884105729").unwrap(),
                "Expected overflow error for I256 to i128 conversion"
            ),
            (
                I256::from_dec_str("170141183460469231731687303715884105728").unwrap(),
                "Expected underflow error for I256 to i128 conversion"
            )
        ];

        for (value, message) in test_values.iter() {
            let result = i256_to_i128(*value);
            assert!(result.is_err(), "{}", message);
        }
    }

    #[test]
    fn test_i128_to_i256_and_back() {
        let test_values = [
            i128::MIN,
            i128::MAX,
            -170141183460469231731687303715884105720_i128,
            170141183460469231731687303715884105727_i128
        ];
        for &original in test_values.iter() {
            let converted = i128_to_i256(original);
            assert_eq!(
                converted,
                I256::from_dec_str(format!("{}", original).as_str()).unwrap(),
                "i128 to I256 conversion failed"
            );
            let back_to_original = i256_to_i128(converted).unwrap();
            assert_eq!(original, back_to_original, "i128 to I256 conversion failed");
        }
    }

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
        let test_values =
            [I24::MIN.as_i32(), -1025_i32, 1024_i32, 1000_i32, -1000_i32, I24::MAX.as_i32()];
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
