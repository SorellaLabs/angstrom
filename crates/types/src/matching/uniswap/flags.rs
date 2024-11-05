// Let's make a nice uniswap address flag builder one day

// let before_swap = U160::from(1_u8) << 7;
//     let before_initialize = U160::from(1_u8) << 13;
//     let before_add_liquidity = U160::from(1_u8) << 11;
//     let before_remove_liquidity = U160::from(1_u8) << 9;
//     let after_remove_liquidity = U160::from(1_u8) << 8;

/*
uint160 internal constant ALL_HOOK_MASK = uint160((1 << 14) - 1);

    uint160 internal constant BEFORE_INITIALIZE_FLAG = 1 << 13;
    uint160 internal constant AFTER_INITIALIZE_FLAG = 1 << 12;

    uint160 internal constant BEFORE_ADD_LIQUIDITY_FLAG = 1 << 11;
    uint160 internal constant AFTER_ADD_LIQUIDITY_FLAG = 1 << 10;

    uint160 internal constant BEFORE_REMOVE_LIQUIDITY_FLAG = 1 << 9;
    uint160 internal constant AFTER_REMOVE_LIQUIDITY_FLAG = 1 << 8;

    uint160 internal constant BEFORE_SWAP_FLAG = 1 << 7;
    uint160 internal constant AFTER_SWAP_FLAG = 1 << 6;

    uint160 internal constant BEFORE_DONATE_FLAG = 1 << 5;
    uint160 internal constant AFTER_DONATE_FLAG = 1 << 4;

    uint160 internal constant BEFORE_SWAP_RETURNS_DELTA_FLAG = 1 << 3;
    uint160 internal constant AFTER_SWAP_RETURNS_DELTA_FLAG = 1 << 2;
    uint160 internal constant AFTER_ADD_LIQUIDITY_RETURNS_DELTA_FLAG = 1 << 1;
    uint160 internal constant AFTER_REMOVE_LIQUIDITY_RETURNS_DELTA_FLAG = 1 << 0;
 */

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
}

impl From<UniswapFlags> for U160 {
    fn from(value: UniswapFlags) -> U160 {
        let bitshift: usize = match value {
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
        Into::<U160>::into(self) | Into::<U160>::into(rhs)
    }
}

impl BitOr<UniswapFlags> for U160 {
    type Output = U160;

    fn bitor(self, rhs: UniswapFlags) -> Self::Output {
        self | Into::<U160>::into(rhs)
    }
}

impl BitOr<U160> for UniswapFlags {
    type Output = U160;

    fn bitor(self, rhs: U160) -> Self::Output {
        Into::<U160>::into(self) | rhs
    }
}
