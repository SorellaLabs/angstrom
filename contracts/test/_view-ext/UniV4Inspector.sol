// SPDX-License-Identifier: UNLICENSED
pragma solidity ^0.8.0;

import {PoolManager} from "v4-core/src/PoolManager.sol";
import {Pool} from "v4-core/src/libraries/Pool.sol";
import {PoolId} from "v4-core/src/types/PoolId.sol";
import {Position} from "v4-core/src/libraries/Position.sol";
import {Slot0} from "v4-core/src/types/Slot0.sol";

/// @author philogy <https://github.com/philogy>
/// @notice This contract is *not* for production use.
contract UniV4Inspector is PoolManager {
    using Position for mapping(bytes32 => Position.State);

    constructor() PoolManager(address(0)) {}

    function getPool(PoolId id)
        external
        view
        returns (
            uint160 sqrtPriceX96,
            int24 tick,
            uint24 protocolFee,
            uint24 lpFee,
            uint256 feeGrowthGlobal0X128,
            uint256 feeGrowthGlobal1X128,
            uint128 liquidity
        )
    {
        Pool.State storage state = _getPool(id);
        Slot0 slot0 = state.slot0;

        sqrtPriceX96 = slot0.sqrtPriceX96();
        tick = slot0.tick();
        protocolFee = slot0.protocolFee();
        lpFee = slot0.lpFee();
        feeGrowthGlobal0X128 = state.feeGrowthGlobal0X128;
        feeGrowthGlobal1X128 = state.feeGrowthGlobal1X128;
        liquidity = state.liquidity;
    }

    function getBitmapWord(PoolId id, int16 wordPos) external view returns (uint256) {
        return _getPool(id).tickBitmap[wordPos];
    }

    function getTick(PoolId id, int24 tick)
        external
        view
        returns (
            uint128 liquidityGross,
            int128 liquidityNet,
            uint256 feeGrowthOutside0X128,
            uint256 feeGrowthOutside1X128
        )
    {
        Pool.TickInfo storage tickInfo = _getPool(id).ticks[tick];
        liquidityGross = tickInfo.liquidityGross;
        liquidityNet = tickInfo.liquidityNet;
        feeGrowthOutside0X128 = tickInfo.feeGrowthOutside0X128;
        feeGrowthOutside1X128 = tickInfo.feeGrowthOutside1X128;
    }

    function getPosition(PoolId id, address owner, int24 tickLower, int24 tickUpper, bytes32 salt)
        external
        view
        returns (
            uint128 liquidity,
            uint256 feeGrowthInside0LastX128,
            uint256 feeGrowthInside1LastX128
        )
    {
        Position.State storage info = _getPool(id).positions.get(owner, tickLower, tickUpper, salt);
        liquidity = info.liquidity;
        feeGrowthInside0LastX128 = info.feeGrowthInside0LastX128;
        feeGrowthInside1LastX128 = info.feeGrowthInside1LastX128;
    }
}
