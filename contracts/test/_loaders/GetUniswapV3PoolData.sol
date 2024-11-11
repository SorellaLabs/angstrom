//SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {IERC20} from "../../lib/forge-std/src/interfaces/IERC20.sol";

contract GetUniswapV3PoolData {
    struct PoolData {
        address tokenA;
        uint8 tokenADecimals;
        address tokenB;
        uint8 tokenBDecimals;
        uint128 liquidity;
        uint160 sqrtPrice;
        int24 tick;
        int24 tickSpacing;
        uint24 fee;
        int128 liquidityNet;
    }

    constructor(address poolAddress) {
        if (codeSizeIsZero(poolAddress)) revert("Invalid pool address");
        PoolData memory poolData;

        poolData.tokenA = IUniswapV3Pool(poolAddress).token0();
        poolData.tokenB = IUniswapV3Pool(poolAddress).token1();
        if (codeSizeIsZero(poolData.tokenA)) revert("Invalid tokenA address");
        if (codeSizeIsZero(poolData.tokenB)) revert("Invalid tokenB address");

        (uint160 sqrtPriceX96, int24 tick,,,,,) = IUniswapV3Pool(poolAddress).slot0();
        (, int128 liquidityNet,,,,,,) = IUniswapV3Pool(poolAddress).ticks(tick);

        poolData.tokenADecimals = IERC20(poolData.tokenA).decimals();
        poolData.tokenBDecimals = IERC20(poolData.tokenB).decimals();
        poolData.liquidity = IUniswapV3Pool(poolAddress).liquidity();
        poolData.tickSpacing = IUniswapV3Pool(poolAddress).tickSpacing();
        poolData.fee = IUniswapV3Pool(poolAddress).fee();
        poolData.sqrtPrice = sqrtPriceX96;
        poolData.tick = tick;
        poolData.liquidityNet = liquidityNet;

        bytes memory abiEncodedData = abi.encode(poolData);
        assembly {
            let dataStart := add(abiEncodedData, 0x20)
            let dataSize := 320
            return(dataStart, dataSize)
        }
    }

    function codeSizeIsZero(address target) internal view returns (bool) {
        return target.code.length == 0;
    }
}

interface IUniswapV3Pool {
    function token0() external view returns (address);

    function token1() external view returns (address);

    function fee() external view returns (uint24);

    function tickSpacing() external view returns (int24);

    function liquidity() external view returns (uint128);

    function slot0()
        external
        view
        returns (
            uint160 sqrtPriceX96,
            int24 tick,
            uint16 observationIndex,
            uint16 observationCardinality,
            uint16 observationCardinalityNext,
            uint8 feeProtocol,
            bool unlocked
        );

    function ticks(int24 tick)
        external
        view
        returns (
            uint128 liquidityGross,
            int128 liquidityNet,
            uint256 feeGrowthOutside0X128,
            uint256 feeGrowthOutside1X128,
            int56 tickCumulativeOutside,
            uint160 secondsPerLiquidityOutsideX128,
            uint32 secondsOutside,
            bool initialized
        );
}
