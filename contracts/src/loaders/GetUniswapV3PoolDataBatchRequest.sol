//SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

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

interface IERC20 {
    function decimals() external view returns (uint8);
}

/**
 * @dev This contract is not meant to be deployed. Instead, use a static call with the
 *       deployment bytecode as payload.
 */
contract GetUniswapV3PoolDataBatchRequest {
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

        // Check that tokenA and tokenB do not have codesize of 0
        if (codeSizeIsZero(poolData.tokenA)) revert("Invalid tokenA address");
        if (codeSizeIsZero(poolData.tokenB)) revert("Invalid tokenB address");

        // Get tokenA decimals
        (bool tokenADecimalsSuccess, bytes memory tokenADecimalsData) =
            poolData.tokenA.call{gas: 20000}(abi.encodeWithSignature("decimals()"));

        if (tokenADecimalsSuccess && tokenADecimalsData.length == 32) {
            uint256 tokenADecimals = abi.decode(tokenADecimalsData, (uint256));
            if (tokenADecimals == 0 || tokenADecimals > 255) revert("Invalid tokenA decimals");
            poolData.tokenADecimals = uint8(tokenADecimals);
        } else {
            revert("Failed to get tokenA decimals");
        }

        // Get tokenB decimals
        (bool tokenBDecimalsSuccess, bytes memory tokenBDecimalsData) =
            poolData.tokenB.call{gas: 20000}(abi.encodeWithSignature("decimals()"));

        if (tokenBDecimalsSuccess && tokenBDecimalsData.length == 32) {
            uint256 tokenBDecimals = abi.decode(tokenBDecimalsData, (uint256));
            if (tokenBDecimals == 0 || tokenBDecimals > 255) revert("Invalid tokenB decimals");
            poolData.tokenBDecimals = uint8(tokenBDecimals);
        } else {
            revert("Failed to get tokenB decimals");
        }

        (uint160 sqrtPriceX96, int24 tick,,,,,) = IUniswapV3Pool(poolAddress).slot0();
        (, int128 liquidityNet,,,,,,) = IUniswapV3Pool(poolAddress).ticks(tick);

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
