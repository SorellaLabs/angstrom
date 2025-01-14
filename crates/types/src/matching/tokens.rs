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
