use std::ops::{Add, Sub};

use alloy::primitives::I256;
use angstrom_types_primitives::primitive::{Quantity, SqrtPriceX96};
use liquidity_base::BaselineLiquidity;
use pool_swap::{PoolSwap, PoolSwapResult};
use serde::{Deserialize, Serialize};

pub mod donation;
pub mod liquidity_base;
pub mod pool_swap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselinePoolState {
    liquidity: BaselineLiquidity,
    block:     u64,
    fee:       u32
}

impl BaselinePoolState {
    pub fn new(liquidity: BaselineLiquidity, block: u64, fee: u32) -> Self {
        Self { liquidity, fee, block }
    }

    pub fn block_number(&self) -> u64 {
        self.block
    }

    pub fn fee(&self) -> u32 {
        self.fee
    }

    pub fn current_tick(&self) -> i32 {
        self.liquidity.start_tick
    }

    pub fn current_liquidity(&self) -> u128 {
        self.liquidity.start_liquidity
    }

    pub fn current_price(&self) -> SqrtPriceX96 {
        self.liquidity.start_sqrt_price
    }

    pub fn noop(&self) -> PoolSwapResult<'_> {
        PoolSwapResult {
            fee:           self.fee,
            start_price:   self.liquidity.start_sqrt_price,
            start_tick:    self.liquidity.start_tick,
            end_price:     self.liquidity.start_sqrt_price,
            end_tick:      self.liquidity.start_tick,
            total_d_t0:    0,
            total_d_t1:    0,
            steps:         vec![],
            end_liquidity: self.liquidity.current()
        }
    }

    pub fn swap_current_with_amount(
        &self,
        amount: I256,
        direction: bool
    ) -> eyre::Result<PoolSwapResult<'_>> {
        let liq = self.liquidity.current();

        PoolSwap { liquidity: liq, target_amount: amount, target_price: None, direction, fee: 0 }
            .swap()
    }

    /// Swap to current price is designed to represent all swap outcomes as an
    /// amount in swap. Because of this, this swap does two swaps to make
    /// sure the values always align perfectly.
    pub fn swap_current_to_price(
        &self,
        price_limit: SqrtPriceX96
    ) -> eyre::Result<PoolSwapResult<'_>> {
        let liq = self.liquidity.current();
        let direction = liq.current_sqrt_price >= price_limit;

        let price_swap = PoolSwap {
            liquidity: liq,
            target_amount: I256::MAX,
            target_price: Some(price_limit),
            direction,
            fee: 0
        }
        .swap()?;

        let amount_in = if direction { price_swap.total_d_t0 } else { price_swap.total_d_t1 };
        let amount = I256::unchecked_from(amount_in);

        self.swap_current_with_amount(amount, direction)
    }

    pub fn swap_current_to_price_raw(
        &self,
        price_limit: SqrtPriceX96
    ) -> eyre::Result<PoolSwapResult<'_>> {
        let liq = self.liquidity.current();

        let direction = liq.current_sqrt_price >= price_limit;

        PoolSwap {
            liquidity: liq,
            target_amount: I256::MAX,
            target_price: Some(price_limit),
            direction,
            fee: 0
        }
        .swap()
    }
}

impl<'a> Add<Quantity> for &'a BaselinePoolState {
    type Output = eyre::Result<PoolSwapResult<'a>>;

    fn add(self, rhs: Quantity) -> Self::Output {
        let amount = I256::unchecked_from(rhs.magnitude());
        let direction = rhs.as_input();
        self.swap_current_with_amount(amount, direction.is_ask())
    }
}

impl<'a> Sub<Quantity> for &'a BaselinePoolState {
    type Output = eyre::Result<PoolSwapResult<'a>>;

    fn sub(self, rhs: Quantity) -> Self::Output {
        let amount = I256::unchecked_from(rhs.magnitude()).saturating_neg();
        let direction = rhs.as_output();
        self.swap_current_with_amount(amount, direction.is_ask())
    }
}
