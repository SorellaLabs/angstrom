//SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

<<<<<<<< HEAD:contracts/test/_loaders/GetUniswapV4PoolData.sol
import {IERC20} from "../../lib/forge-std/src/interfaces/IERC20.sol";
import {IPoolManager} from "../../lib/v4-core/src/interfaces/IPoolManager.sol";
import {PoolId} from "../../lib/v4-core/src/types/PoolId.sol";
import {Slot0} from "../../lib/v4-core/src/types/Slot0.sol";
import {IUniV4} from "src/interfaces/IUniV4.sol";
========
import {IERC20} from "forge-std/interfaces/IERC20.sol";
import {IPoolManager} from "v4-core/src/interfaces/IPoolManager.sol";
import {PoolId} from "v4-core/src/types/PoolId.sol";
import {Slot0} from "v4-core/src/types/Slot0.sol";
import {IUniV4} from "core/src/interfaces/IUniV4.sol";
>>>>>>>> main:crates/uniswap-v4/src/uniswap/loaders/GetUniswapV4PoolData.sol

contract GetUniswapV4PoolData {
    struct PoolData {
        uint8 token0Decimals;
        uint8 token1Decimals;
        uint128 liquidity;
        uint160 sqrtPrice;
        int24 tick;
        int128 liquidityNet;
    }

    constructor(PoolId poolId, address poolManager, address asset0, address asset1) {
        if (codeSizeIsZero(poolManager)) revert("Invalid pool address");
        PoolData memory poolData;
        Slot0 slot0 = IUniV4.getSlot0(IPoolManager(poolManager), poolId);
        uint128 liquidity = IUniV4.getPoolLiquidity(IPoolManager(poolManager), poolId);
        (, int128 liquidityNet) =
            IUniV4.getTickLiquidity(IPoolManager(poolManager), poolId, slot0.tick());

        poolData.token0Decimals = IERC20(asset0).decimals();
        poolData.token1Decimals = IERC20(asset1).decimals();
        poolData.liquidity = liquidity;
        poolData.sqrtPrice = slot0.sqrtPriceX96();
        poolData.tick = slot0.tick();
        poolData.liquidityNet = liquidityNet;

        bytes memory abiEncodedData = abi.encode(poolData);
        assembly {
            let dataStart := add(abiEncodedData, 0x20)
            let dataSize := 192
            return(dataStart, dataSize)
        }
    }

    function codeSizeIsZero(address target) internal view returns (bool) {
        return target.code.length == 0;
    }
}
