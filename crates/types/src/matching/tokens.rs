use std::ops::{Add, Deref, Sub};

use alloy::primitives::U256;

use super::Ray;

#[derive(Copy, Clone)]
pub enum TokenQuantity {
    Token0(u128),
    Token1(u128)
}

impl Deref for TokenQuantity {
    type Target = u128;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Token0(q) | Self::Token1(q) => q
        }
    }
}

impl Add<u128> for TokenQuantity {
    type Output = Self;

    fn add(self, rhs: u128) -> Self::Output {
        match self {
            Self::Token0(q) => Self::Token0(q.add(rhs)),
            Self::Token1(q) => Self::Token1(q.add(rhs))
        }
    }
}

impl Sub<u128> for TokenQuantity {
    type Output = Self;

    fn sub(self, rhs: u128) -> Self::Output {
        match self {
            Self::Token0(q) => Self::Token0(q.sub(rhs)),
            Self::Token1(q) => Self::Token1(q.sub(rhs))
        }
    }
}

impl TokenQuantity {
    /// A new quantity of Token0
    pub fn zero<T: Into<u128>>(source: T) -> Self {
        Self::Token0(source.into())
    }

    /// A new quantity of Token1
    pub fn one<T: Into<u128>>(source: T) -> Self {
        Self::Token1(source.into())
    }

    /// A quantity of Token0 from the U256 format specifically
    pub fn zero_from_uint(source: U256) -> Self {
        Self::Token0(source.to())
    }

    /// A quantity of Token1 from the U256 format specifically
    pub fn one_from_uint(source: U256) -> Self {
        Self::Token1(source.to())
    }

    pub fn swap_at_price(&self, price: Ray) -> Self {
        match self {
            Self::Token0(q) => Self::Token1(price.mul_quantity(U256::from(*q)).to()),
            Self::Token1(q) => Self::Token0(price.inverse_quantity(*q, true))
        }
    }

    pub fn as_t0(&self, swap_price: Ray) -> Self {
        match self {
            Self::Token0(_) => *self,
            Self::Token1(_) => self.swap_at_price(swap_price)
        }
    }

    pub fn as_t1(&self, swap_price: Ray) -> Self {
        match self {
            Self::Token0(_) => self.swap_at_price(swap_price),
            Self::Token1(_) => *self
        }
    }

    pub fn quantity(&self) -> u128 {
        match self {
            Self::Token0(q) | Self::Token1(q) => *q
        }
    }

    pub fn as_u256(&self) -> U256 {
        match self {
            Self::Token0(q) | Self::Token1(q) => U256::from(*q)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_quantity_creation() {
        // Test creation with u128
        let t0 = TokenQuantity::zero(100u128);
        let t1 = TokenQuantity::one(200u128);

        assert_eq!(*t0, 100u128);
        assert_eq!(*t1, 200u128);

        // Test creation with smaller types
        let t0_small = TokenQuantity::zero(50u32);
        let t1_small = TokenQuantity::one(75u16);

        assert_eq!(*t0_small, 50u128);
        assert_eq!(*t1_small, 75u128);
    }

    #[test]
    fn test_token_quantity_from_uint() {
        let uint_value = U256::from(1000u64);

        let t0 = TokenQuantity::zero_from_uint(uint_value);
        let t1 = TokenQuantity::one_from_uint(uint_value);

        assert_eq!(*t0, 1000u128);
        assert_eq!(*t1, 1000u128);
    }

    #[test]
    fn test_token_quantity_addition() {
        let t0 = TokenQuantity::zero(100u128);
        let t1 = TokenQuantity::one(200u128);

        let t0_added = t0 + 50u128;
        let t1_added = t1 + 75u128;

        assert_eq!(*t0_added, 150u128);
        assert_eq!(*t1_added, 275u128);

        // Test addition with max values
        let max_token = TokenQuantity::zero(u128::MAX - 1);
        let max_added = max_token + 1u128;
        assert_eq!(*max_added, u128::MAX);
    }

    #[test]
    #[should_panic]
    fn test_token_quantity_addition_overflow() {
        let max_token = TokenQuantity::zero(u128::MAX);
        let _overflow = max_token + 1u128;
    }

    #[test]
    fn test_token_quantity_subtraction() {
        let t0 = TokenQuantity::zero(100u128);
        let t1 = TokenQuantity::one(200u128);

        let t0_subbed = t0 - 50u128;
        let t1_subbed = t1 - 75u128;

        assert_eq!(*t0_subbed, 50u128);
        assert_eq!(*t1_subbed, 125u128);
    }

    #[test]
    #[should_panic]
    fn test_token_quantity_subtraction_underflow() {
        let token = TokenQuantity::zero(10u128);
        let _underflow = token - 20u128;
    }

    #[test]
    fn test_swap_at_price() {
        let quantity = 1000u128;
        let price_value = U256::from(2) * U256::from(10).pow(U256::from(27)); // 2.0 in Ray format
        let price = Ray::from(price_value);

        // Test Token0 to Token1 swap
        let t0 = TokenQuantity::zero(quantity);
        let swapped_to_t1 = t0.swap_at_price(price);
        match swapped_to_t1 {
            TokenQuantity::Token1(q) => assert_eq!(q, 2000), // 1000 * 2.0 = 2000
            _ => panic!("Wrong token type after swap")
        }

        // Test Token1 to Token0 swap
        let t1 = TokenQuantity::one(quantity);
        let swapped_to_t0 = t1.swap_at_price(price);
        match swapped_to_t0 {
            TokenQuantity::Token0(q) => assert_eq!(q, 500), // 1000 / 2.0 = 500
            _ => panic!("Wrong token type after swap")
        }
    }

    #[test]
    fn test_as_t0_and_as_t1() {
        let price_value = U256::from(2) * U256::from(10).pow(U256::from(27)); // 2.0 in Ray format
        let price = Ray::from(price_value);

        // Test Token0 conversions
        let t0 = TokenQuantity::zero(1000u128);
        let t0_as_t0 = t0.as_t0(price);
        let t0_as_t1 = t0.as_t1(price);

        assert_eq!(*t0_as_t0, 1000u128); // Should remain unchanged
        assert_eq!(*t0_as_t1, 2000u128); // Should be converted to Token1

        // Test Token1 conversions
        let t1 = TokenQuantity::one(1000u128);
        let t1_as_t0 = t1.as_t0(price);
        let t1_as_t1 = t1.as_t1(price);

        assert_eq!(*t1_as_t0, 500u128); // Should be converted to Token0
        assert_eq!(*t1_as_t1, 1000u128); // Should remain unchanged
    }

    #[test]
    fn test_quantity_and_as_u256() {
        let value = 12345u128;
        let t0 = TokenQuantity::zero(value);
        let t1 = TokenQuantity::one(value);

        // Test quantity()
        assert_eq!(t0.quantity(), value);
        assert_eq!(t1.quantity(), value);

        // Test as_u256()
        assert_eq!(t0.as_u256(), U256::from(value));
        assert_eq!(t1.as_u256(), U256::from(value));
    }

    #[test]
    fn test_deref() {
        let t0 = TokenQuantity::zero(100u128);
        let t1 = TokenQuantity::one(200u128);

        assert_eq!(*t0, 100u128);
        assert_eq!(*t1, 200u128);
    }

    #[test]
    fn test_copy_clone() {
        let original = TokenQuantity::zero(100u128);
        let copied = original;
        let cloned = original.clone();

        assert_eq!(*original, 100u128);
        assert_eq!(*copied, 100u128);
        assert_eq!(*cloned, 100u128);
    }
}
