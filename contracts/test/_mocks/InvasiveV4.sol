// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {PoolManager} from "v4-core/src/PoolManager.sol";
import {PoolId} from "v4-core/src/types/PoolId.sol";
import {Slot0} from "v4-core/src/types/Slot0.sol";
import {Pool} from "v4-core/src/libraries/Pool.sol";
import {Position} from "v4-core/src/libraries/Position.sol";

/// @author philogy <https://github.com/philogy>
contract InvasiveV4 is PoolManager {
    using Position for mapping(bytes32 => Position.State);

    function getPoolState(PoolId id)
        public
        view
        returns (
            Slot0 slot0,
            uint256 feeGrowthGlobal0X128,
            uint256 feeGrowthGlobal1X128,
            uint128 liquidity
        )
    {
        Pool.State storage state = _pools[id];
        slot0 = state.slot0;
        feeGrowthGlobal0X128 = state.feeGrowthGlobal0X128;
        feeGrowthGlobal1X128 = state.feeGrowthGlobal1X128;
        liquidity = state.liquidity;
    }

    function setPoolSlot0(PoolId id, Slot0 slot0) public {
        _pools[id].slot0 = slot0;
    }

    function setPoolFeeGrowthGlobal0X128(PoolId id, uint256 feeGrowthGlobal0X128) public {
        _pools[id].feeGrowthGlobal0X128 = feeGrowthGlobal0X128;
    }

    function setPoolFeeGrowthGlobal1X128(PoolId id, uint256 feeGrowthGlobal1X128) public {
        _pools[id].feeGrowthGlobal1X128 = feeGrowthGlobal1X128;
    }

    function setPoolLiquidity(PoolId id, uint128 liquidity) public {
        _pools[id].liquidity = liquidity;
    }

    function getTickInfo(PoolId id, int24 tick) public view returns (Pool.TickInfo memory) {
        return _pools[id].ticks[tick];
    }

    function setTickInfo(PoolId id, int24 tick, Pool.TickInfo calldata info) public {
        _pools[id].ticks[tick] = info;
    }

    function getBitmapWord(PoolId id, int16 wordPos) public view returns (uint256) {
        return _pools[id].tickBitmap[wordPos];
    }

    function setBitmapWord(PoolId id, int16 wordPos, uint256 word) public {
        _pools[id].tickBitmap[wordPos] = word;
    }

    function getPosition(PoolId id, address owner, int24 tickLower, int24 tickUpper, bytes32 salt)
        public
        view
        returns (Position.State memory)
    {
        return _pools[id].positions.get(owner, tickLower, tickUpper, salt);
    }

    function getPosition(PoolId id, bytes32 key) public view returns (Position.State memory) {
        return _pools[id].positions[key];
    }

    function setPosition(PoolId id, bytes32 key, Position.State calldata position) public {
        _pools[id].positions[key] = position;
    }
}
