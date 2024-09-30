// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "test/_helpers/BaseTest.sol";
import {InvasiveV4} from "test/_mocks/InvasiveV4.sol";
import {IUniV4 as PoolManagerStateGetter} from "src/interfaces/IUniV4.sol";

import {PoolId} from "v4-core/src/types/PoolId.sol";
import {Pool} from "v4-core/src/libraries/Pool.sol";
import {Slot0} from "v4-core/src/types/Slot0.sol";

import {Position} from "v4-core/src/libraries/Position.sol";
import {Positions} from "src/libraries/Positions.sol";

/// @author philogy <https://github.com/philogy>
contract PoolManagerStateGetterTest is BaseTest {
    InvasiveV4 uni;
    Positions positions;

    function setUp() public {
        uni = new InvasiveV4();
    }

    function test_fuzzing_getSlot0(PoolId id, Slot0 slot0) public {
        uni.setPoolSlot0(id, slot0);

        (Slot0 expectedSlot0,,,) = uni.getPoolState(id);
        assertEq(expectedSlot0, slot0, "sanity check");
        Slot0 retrievedSlot0 = PoolManagerStateGetter.getSlot0(uni, id);
        assertEq(retrievedSlot0, expectedSlot0, "retrieved != expected");
    }

    function test_fuzzing_getPoolLiquidity(PoolId id, uint128 liquidity) public {
        uni.setPoolLiquidity(id, liquidity);

        (,,, uint128 expectedLiquidity) = uni.getPoolState(id);
        assertEq(expectedLiquidity, liquidity, "sanity check");
        uint128 retrievedLiquidity = PoolManagerStateGetter.getPoolLiquidity(uni, id);
        assertEq(retrievedLiquidity, expectedLiquidity, "retrieved != expected");
    }

    function test_fuzzing_getPoolBitmapInfo(PoolId id, int16 wordPos, uint256 word) public {
        uni.setBitmapWord(id, wordPos, word);

        uint256 expectedWord = uni.getBitmapWord(id, wordPos);
        assertEq(expectedWord, word, "sanity check");
        uint256 retrievedWord = PoolManagerStateGetter.getPoolBitmapInfo(uni, id, wordPos);
        assertEq(retrievedWord, expectedWord, "retrieved != expected");
    }

    function test_fuzzing_getTickLiquidity(
        PoolId id,
        int24 tick,
        uint128 liquidityGross,
        int128 liquidityNet
    ) public {
        Pool.TickInfo memory info;
        info.liquidityGross = liquidityGross;
        info.liquidityNet = liquidityNet;
        uni.setTickInfo(id, tick, info);

        Pool.TickInfo memory retrievedInfo = uni.getTickInfo(id, tick);
        assertEq(retrievedInfo.liquidityGross, liquidityGross, "sanity check (1)");
        assertEq(retrievedInfo.liquidityNet, liquidityNet, "sanity check (2)");

        (uint128 retrievedLiquidityGross, int128 retreivedLiquidityNet) =
            PoolManagerStateGetter.getTickLiquidity(uni, id, tick);
        assertEq(
            retrievedLiquidityGross,
            retrievedInfo.liquidityGross,
            "retrieved != expected (liquidity gross)"
        );
        assertEq(
            retreivedLiquidityNet,
            retrievedInfo.liquidityNet,
            "retrieved != expected (liquidity net)"
        );
    }

    function test_fuzzing_getPositionLiquidity(
        PoolId id,
        address owner,
        int24 lowerTick,
        int24 upperTick,
        bytes32 salt,
        uint128 liquidity
    ) public {
        (, bytes32 positionKey) = positions.get(id, owner, lowerTick, upperTick, salt);

        Position.State memory newState;
        newState.liquidity = liquidity;
        uni.setPosition(id, positionKey, newState);

        Position.State memory state = uni.getPosition(id, owner, lowerTick, upperTick, salt);
        assertEq(state.liquidity, liquidity, "setting failed");
        assertEq(
            PoolManagerStateGetter.getPositionLiquidity(uni, id, positionKey),
            liquidity,
            "retrieved != expected (position liquidity)"
        );
    }

    function assertEq(Slot0 a, Slot0 b, string memory error) internal pure {
        assertEq(Slot0.unwrap(a), Slot0.unwrap(b), error);
    }
}
