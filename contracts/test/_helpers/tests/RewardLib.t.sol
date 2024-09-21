// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "test/_helpers/BaseTest.sol";
import {RewardLib, TickReward} from "../RewardLib.sol";
import {PoolManager} from "v4-core/src/PoolManager.sol";
import {PoolId} from "v4-core/src/types/PoolId.sol";
import {TickLib} from "src/libraries/TickLib.sol";
import {TICK_SPACING} from "src/Constants.sol";
import {SuperConversionLib} from "super-sol/libraries/SuperConversionLib.sol";
import {RewardsUpdate} from "../../../src/reference/PoolUpdate.sol";

import {FormatLib} from "super-sol/libraries/FormatLib.sol";
import {console} from "forge-std/console.sol";

PoolId constant id = PoolId.wrap(0);

contract FakeUni is PoolManager {
    using FormatLib for *;
    using TickLib for int24;

    mapping(int24 => uint128) public tickLiq;

    function setCurrentTick(int24 tick) public {
        _pools[id].slot0 = _pools[id].slot0.setTick(tick);
        _pools[id].liquidity = tickLiq[tick.normalize()];
    }

    function liq() public view returns (uint128) {
        return _pools[id].liquidity;
    }

    function addLiquidity(int24 lowerTick, int24 upperTick, uint128 liquidity) public {
        _initialize(lowerTick);
        _initialize(upperTick);
        _pools[id].ticks[lowerTick].liquidityNet += int128(liquidity);
        _pools[id].ticks[upperTick].liquidityNet -= int128(liquidity);
        int24 tick = _pools[id].slot0.tick();

        if (lowerTick <= tick && tick < upperTick) {
            _pools[id].liquidity += liquidity;
        }

        for (tick = lowerTick; tick < upperTick; tick += TICK_SPACING) {
            tickLiq[tick] += liquidity;
        }
    }

    function _initialize(int24 tick) internal {
        (int16 wordPos, uint8 bitPos) = TickLib.position(TickLib.compress(tick));
        _pools[id].tickBitmap[wordPos] |= (1 << uint256(bitPos));
    }
}

/// @author philogy <https://github.com/philogy>
/// @dev Reward utility so brittle and crucial to testing that it was a good idea to have a test to
/// see where/if it breaks.
contract RewardLibTest is BaseTest {
    using FormatLib for *;
    using SuperConversionLib for *;

    FakeUni uni;

    int24 tick;

    bool constant ABOVE = false;
    bool constant BELOW = true;

    function setUp() public {
        uni = new FakeUni();
        uni.setCurrentTick(0);
        uni.addLiquidity(-120, 0, 1e21);
        uni.addLiquidity(0, 120, 1e21);
        uni.addLiquidity(120, 180, 0.23e21);
        uni.addLiquidity(-180, 60, 0.0083e21);
        uni.addLiquidity(-600, -540, 0.83e21);
    }

    function test_fuzzing_currentOnlyUpdate_tickAtBoundary(uint128 amount) public {
        uni.setCurrentTick(tick = 0);
        assertCreatesUpdates(re(TickReward(tick, amount)), RewardsUpdate(ABOVE, tick, uni.liq(), [amount].into()));

        uni.setCurrentTick(tick = 60);
        assertCreatesUpdates(re(TickReward(tick, amount)), RewardsUpdate(ABOVE, tick, uni.liq(), [amount].into()));

        uni.setCurrentTick(tick = -120);
        assertCreatesUpdates(re(TickReward(tick, amount)), RewardsUpdate(ABOVE, tick, uni.liq(), [amount].into()));
    }

    function test_fuzzing_currentOnlyUpdate_tickInInitRange(uint128 amount) public {
        uni.setCurrentTick(tick = -118);
        assertCreatesUpdates(re(TickReward(-120, amount)), RewardsUpdate(ABOVE, -120, uni.liq(), [amount].into()));

        uni.setCurrentTick(tick = -121);
        assertCreatesUpdates(re(TickReward(-180, amount)), RewardsUpdate(ABOVE, -180, uni.liq(), [amount].into()));
    }

    function test_fuzzing_rewardBelow_onlyOne_tickAtBoundary(uint128 amount) public {
        uni.setCurrentTick(tick = 0);
        assertCreatesUpdates(
            re(TickReward(-120, amount)), RewardsUpdate(BELOW, 0, uni.tickLiq(-120), [amount, 0].into())
        );

        uni.setCurrentTick(tick = 60);
        assertCreatesUpdates(re(TickReward(0, amount)), RewardsUpdate(BELOW, 60, uni.tickLiq(0), [amount, 0].into()));
    }

    function test_fuzzing_rewardBelow_onlyOneAway_tickAtBoundary(uint128 amount) public {
        uni.setCurrentTick(tick = 0);
        assertCreatesUpdates(
            re(TickReward(-180, amount)), RewardsUpdate(BELOW, -120, uni.tickLiq(-180), [amount, 0, 0].into())
        );

        uni.setCurrentTick(tick = 60);
        assertCreatesUpdates(
            re(TickReward(-120, amount)), RewardsUpdate(BELOW, 0, uni.tickLiq(-120), [amount, 0, 0].into())
        );
    }

    function test_fuzzing_rewardBelow_tickInUninitRange(uint128 amount1, uint128 amount2) public {
        uni.setCurrentTick(tick = -58);

        assertCreatesUpdates(
            re(TickReward(-180, amount1), TickReward(-120, amount2)),
            RewardsUpdate(BELOW, -120, uni.tickLiq(-180), [amount1, amount2].into())
        );

        assertCreatesUpdates(
            re(TickReward(-600, amount1), TickReward(-180, amount2)),
            RewardsUpdate(BELOW, -540, uni.tickLiq(-600), [amount1, 0, amount2, 0].into())
        );
    }

    function test_fuzzing_rewardBelow_onlyOne_tickInInitRange(uint128 amount) public {
        uni.setCurrentTick(tick = 34);
        assertCreatesUpdates(re(TickReward(-120, amount)), RewardsUpdate(BELOW, 0, uni.liq(), [amount, 0].into()));
    }

    function test_fuzzing_rewardBelow_tickOutOfRange(uint128 amount) public {
        uni.setCurrentTick(tick = -181);
        assertCreatesUpdates(
            re(TickReward(-600, amount)), RewardsUpdate(BELOW, -540, uni.tickLiq(-600), [amount, 0].into())
        );
    }

    function test_fuzzing_rewardAbove_single_tickAtBoundary(uint128 amount) public {
        uni.setCurrentTick(tick = 0);
        assertCreatesUpdates(re(TickReward(60, amount)), RewardsUpdate(ABOVE, 60, uni.tickLiq(60), [amount, 0].into()));
    }

    function test_fuzzing_rewardAbove_many_tickAtBoundary(uint128 amount1, uint128 amount2) public {
        uni.setCurrentTick(tick = -120);
        assertCreatesUpdates(
            re(TickReward(0, amount1), TickReward(60, amount2)),
            RewardsUpdate(ABOVE, 60, uni.tickLiq(60), [amount2, amount1, 0].into())
        );
    }

    function test_fuzzing_rewardAbove_single_tickInInitRange(uint128 amount) public {
        uni.setCurrentTick(tick = 34);
        assertCreatesUpdates(re(TickReward(60, amount)), RewardsUpdate(ABOVE, 60, uni.tickLiq(60), [amount, 0].into()));
    }

    function test_fuzzing_rewardAbove_many_tickInInitRange(uint128 amount1, uint128 amount2) public {
        uni.setCurrentTick(tick = -61);
        assertCreatesUpdates(
            re(TickReward(0, amount1), TickReward(60, amount2)),
            RewardsUpdate(ABOVE, 60, uni.tickLiq(60), [amount2, amount1, 0].into())
        );
    }

    function test_fuzzing_rewardAbove_many_tickInUninitRange(uint128 amount1, uint128 amount2) public {
        uni.setCurrentTick(tick = -30);
        assertCreatesUpdates(
            re(TickReward(0, amount1), TickReward(60, amount2)),
            RewardsUpdate(ABOVE, 60, uni.tickLiq(60), [amount2, amount1, 0].into())
        );
    }

    function test_fuzzing_rewardAbove_many_tickOutOfRange(uint128 amount1, uint128 amount2, uint128 amount3) public {
        uni.setCurrentTick(tick = -181);
        assertCreatesUpdates(
            re(TickReward(-180, amount1), TickReward(-120, amount2), TickReward(120, amount3)),
            RewardsUpdate(ABOVE, 120, uni.tickLiq(120), [amount3, 0, 0, amount2, amount1, 0].into())
        );
    }

    function assertCreatesUpdates(TickReward[] memory r, RewardsUpdate memory expected) internal view {
        RewardsUpdate[] memory updates = RewardLib.toUpdates(r, uni, id);
        assertEq(updates.length, 1, "Expected update");
        RewardsUpdate memory update = updates[0];
        assertEq(update.below, expected.below, "below: given != expected");
        assertEq(update.startTick, expected.startTick, "startTick: given != expected");
        assertEq(update.startLiquidity, expected.startLiquidity, "startLiquidity: given != expected");

        string memory arrayNotEqualStr =
            string.concat("(", update.quantities.toStr(), " != ", expected.quantities.toStr(), ")");

        assertEq(
            update.quantities.length,
            expected.quantities.length,
            string.concat("quantities.length: given != expected ", arrayNotEqualStr)
        );
        for (uint256 i = 0; i < update.quantities.length; i++) {
            assertEq(
                update.quantities[i],
                expected.quantities[i],
                string.concat("quantities[", i.toStr(), "]: given != expected ", arrayNotEqualStr)
            );
        }
    }

    function re(TickReward memory reward) internal pure returns (TickReward[] memory r) {
        r = new TickReward[](1);
        r[0] = reward;
    }

    function re(TickReward memory r1, TickReward memory r2) internal pure returns (TickReward[] memory r) {
        r = new TickReward[](2);
        r[0] = r1;
        r[1] = r2;
    }

    function re(TickReward memory r1, TickReward memory r2, TickReward memory r3)
        internal
        pure
        returns (TickReward[] memory r)
    {
        r = new TickReward[](3);
        r[0] = r1;
        r[1] = r2;
        r[2] = r3;
    }
}
