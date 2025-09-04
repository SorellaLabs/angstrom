// // Let's make a nice uniswap address flag builder one day

use std::ops::BitOr;

use alloy::primitives::U160;
pub enum UniswapFlags {
    BeforeInitialize,
    AfterInitialize,
    BeforeAddLiquidity,
    AfterAddLiquidity,
    BeforeRemoveLiquidity,
    AfterRemoveLiquidity,
    BeforeSwap,
    AfterSwap,
    BeforeDonate,
    AfterDonate,
    BeforeSwapReturnsDelta,
    AfterSwapReturnsDelta,
    AfterAddLiquidityReturnsDelta,
    AfterRemoveLiquidityReturnsDelta
}

impl UniswapFlags {
    pub fn mask() -> U160 {
        (U160::from(1_u8) << 14) - U160::from(1_u8)
    }

    pub fn flag(&self) -> U160 {
        let bitshift: usize = match self {
            UniswapFlags::BeforeInitialize => 13,
            UniswapFlags::AfterInitialize => 12,
            UniswapFlags::BeforeAddLiquidity => 11,
            UniswapFlags::AfterAddLiquidity => 10,
            UniswapFlags::BeforeRemoveLiquidity => 9,
            UniswapFlags::AfterRemoveLiquidity => 8,
            UniswapFlags::BeforeSwap => 7,
            UniswapFlags::AfterSwap => 6,
            UniswapFlags::BeforeDonate => 5,
            UniswapFlags::AfterDonate => 4,
            UniswapFlags::BeforeSwapReturnsDelta => 3,
            UniswapFlags::AfterSwapReturnsDelta => 2,
            UniswapFlags::AfterAddLiquidityReturnsDelta => 1,
            UniswapFlags::AfterRemoveLiquidityReturnsDelta => 0
        };
        U160::from(1_u8) << bitshift
    }
}

impl BitOr for UniswapFlags {
    type Output = U160;

    fn bitor(self, rhs: Self) -> Self::Output {
        self.flag() | rhs.flag()
    }
}

impl BitOr<UniswapFlags> for U160 {
    type Output = U160;

    fn bitor(self, rhs: UniswapFlags) -> Self::Output {
        self | rhs.flag()
    }
}

impl BitOr<U160> for UniswapFlags {
    type Output = U160;

    fn bitor(self, rhs: U160) -> Self::Output {
        self.flag() | rhs
    }
}
