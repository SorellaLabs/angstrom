// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "test/_helpers/BaseTest.sol";
import {RewardLib, TickReward} from "../RewardLib.sol";
import {PoolManager} from "v4-core/src/PoolManager.sol";
import {PoolId} from "v4-core/src/types/PoolId.sol";
import {TickLib} from "src/libraries/TickLib.sol";
import {SuperConversionLib} from "super-sol/libraries/SuperConversionLib.sol";
import {RewardsUpdate} from "test/_reference/PoolUpdate.sol";

import {FormatLib} from "super-sol/libraries/FormatLib.sol";
import {console} from "forge-std/console.sol";

PoolId constant id = PoolId.wrap(0);

int24 constant TICK_SPACING = 60;

contract FakeUni is PoolManager {
    using FormatLib for *;
    using TickLib for int24;

    mapping(int24 => uint128) public tickLiq;

    constructor() PoolManager(address(0)) {}

    function setCurrentTick(int24 tick) public {
        _pools[id].slot0 = _pools[id].slot0.setTick(tick);
        _pools[id].liquidity = tickLiq[tick.normalizeUnchecked(TICK_SPACING)];
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
        (int16 wordPos, uint8 bitPos) = TickLib.position(TickLib.compress(tick, TICK_SPACING));
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
        assertCreatesUpdates(re(TickReward(tick, amount)), CurrentOnlyReward(amount));

        uni.setCurrentTick(tick = 60);
        assertCreatesUpdates(re(TickReward(tick, amount)), CurrentOnlyReward(amount));

        uni.setCurrentTick(tick = -120);
        assertCreatesUpdates(re(TickReward(tick, amount)), CurrentOnlyReward(amount));
    }

    function test_fuzzing_currentOnlyUpdate_tickInUninitializedRange(uint128 amount) public {
        uni.setCurrentTick(tick = -1);

        assertCreatesUpdates(re(TickReward(-120, amount)), CurrentOnlyReward(amount));
    }

    function test_fuzzing_currentOnlyUpdate_tickInInitRange(uint128 amount) public {
        uni.setCurrentTick(tick = -118);
        assertCreatesUpdates(re(TickReward(-120, amount)), CurrentOnlyReward(amount));

        uni.setCurrentTick(tick = -121);
        assertCreatesUpdates(re(TickReward(-180, amount)), CurrentOnlyReward(amount));
    }

    function test_fuzzing_rewardBelow_onlyOne_tickAtBoundary(uint128 amount) public {
        uni.setCurrentTick(tick = 0);
        assertCreatesUpdates(
            re(TickReward(-120, amount)), MultiTickReward(0, uni.tickLiq(-120), [amount, 0].into())
        );

        uni.setCurrentTick(tick = 60);
        assertCreatesUpdates(
            re(TickReward(0, amount)), MultiTickReward(60, uni.tickLiq(0), [amount, 0].into())
        );
    }

    function test_fuzzing_rewardBelow_onlyOneAway_tickAtBoundary(uint128 amount) public {
        uni.setCurrentTick(tick = 0);
        assertCreatesUpdates(
            re(TickReward(-180, amount)),
            MultiTickReward(-120, uni.tickLiq(-180), [amount, 0, 0].into())
        );

        uni.setCurrentTick(tick = 60);
        assertCreatesUpdates(
            re(TickReward(-120, amount)),
            MultiTickReward(0, uni.tickLiq(-120), [amount, 0, 0].into())
        );
    }

    function test_fuzzing_rewardBelow_tickInUninitRange(uint128 amount1, uint128 amount2) public {
        uni.setCurrentTick(tick = -58);

        assertCreatesUpdates(
            re(TickReward(-180, amount1), TickReward(-120, amount2)),
            MultiTickReward(-120, uni.tickLiq(-180), [amount1, amount2].into())
        );

        assertCreatesUpdates(
            re(TickReward(-600, amount1), TickReward(-180, amount2)),
            MultiTickReward(-540, uni.tickLiq(-600), [amount1, 0, amount2, 0].into())
        );
    }

    function test_fuzzing_rewardBelow_onlyOne_tickInInitRange(uint128 amount) public {
        uni.setCurrentTick(tick = 34);
        assertCreatesUpdates(
            re(TickReward(-120, amount)), MultiTickReward(0, uni.liq(), [amount, 0].into())
        );
    }

    function test_fuzzing_rewardBelow_tickOutOfRange(uint128 amount) public {
        uni.setCurrentTick(tick = -181);
        assertCreatesUpdates(
            re(TickReward(-600, amount)),
            MultiTickReward(-540, uni.tickLiq(-600), [amount, 0].into())
        );
    }

    function test_fuzzing_rewardAbove_single_tickAtBoundary(uint128 amount) public {
        uni.setCurrentTick(tick = 0);
        assertCreatesUpdates(
            re(TickReward(60, amount)), MultiTickReward(60, uni.tickLiq(60), [amount, 0].into())
        );
    }

    function test_fuzzing_rewardAbove_many_tickAtBoundary(uint128 amount1, uint128 amount2)
        public
    {
        uni.setCurrentTick(tick = -120);
        assertCreatesUpdates(
            re(TickReward(0, amount1), TickReward(60, amount2)),
            MultiTickReward(60, uni.tickLiq(60), [amount2, amount1, 0].into())
        );
    }

    function test_fuzzing_rewardAbove_single_tickInInitRange(uint128 amount) public {
        uni.setCurrentTick(tick = 34);
        assertCreatesUpdates(
            re(TickReward(60, amount)), MultiTickReward(60, uni.tickLiq(60), [amount, 0].into())
        );
    }

    function test_fuzzing_rewardAbove_many_tickInInitRange(uint128 amount1, uint128 amount2)
        public
    {
        uni.setCurrentTick(tick = -61);
        assertCreatesUpdates(
            re(TickReward(0, amount1), TickReward(60, amount2)),
            MultiTickReward(60, uni.tickLiq(60), [amount2, amount1, 0].into())
        );
    }

    function test_fuzzing_rewardAbove_many_tickInUninitRange(uint128 amount1, uint128 amount2)
        public
    {
        uni.setCurrentTick(tick = -30);
        assertCreatesUpdates(
            re(TickReward(0, amount1), TickReward(60, amount2)),
            MultiTickReward(60, uni.tickLiq(60), [amount2, amount1, 0].into())
        );
    }

    function test_fuzzing_rewardAbove_many_includingCurrent(
        uint128 amount1,
        uint128 amount2,
        uint128 amount3
    ) public {
        uni.setCurrentTick(tick = -30);
        assertCreatesUpdates(
            re(TickReward(120, amount1), TickReward(0, amount2), TickReward(-120, amount3)),
            MultiTickReward(120, uni.tickLiq(120), [amount1, 0, amount2, amount3].into())
        );
    }

    function test_fuzzing_rewardAbove_many_tickOutOfRange(
        uint128 amount1,
        uint128 amount2,
        uint128 amount3
    ) public {
        uni.setCurrentTick(tick = -181);
        assertCreatesUpdates(
            re(TickReward(-180, amount1), TickReward(-120, amount2), TickReward(120, amount3)),
            MultiTickReward(120, uni.tickLiq(120), [amount3, 0, 0, amount2, amount1, 0].into())
        );
    }

    function assertCreatesUpdates(TickReward[] memory r, RewardsUpdate memory expected)
        internal
        view
    {
        RewardsUpdate[] memory updates = RewardLib.toUpdates(r, uni, id, TICK_SPACING);
        assertEq(updates.length, 1, "length != 1: Expected single update");
        RewardsUpdate memory update = updates[0];
        assertTrue(
            update.onlyCurrent == expected.onlyCurrent
                && (
                    update.onlyCurrent
                        ? (update.onlyCurrentQuantity == expected.onlyCurrentQuantity)
                        : (
                            update.startTick == expected.startTick
                                && update.startLiquidity == expected.startLiquidity
                                && update.quantities.length == expected.quantities.length
                                && arraysEqual(update.quantities, expected.quantities)
                        )
                ),
            string.concat("returned != expected: ", update.toStr(), " != ", expected.toStr())
        );
    }

    function arraysEqual(uint128[] memory x1, uint128[] memory x2) internal pure returns (bool) {
        if (x1.length != x2.length) return false;
        for (uint256 i = 0; i < x1.length; i++) {
            if (x1[i] != x2[i]) return false;
        }
        return true;
    }

    function CurrentOnlyReward(uint128 amount)
        internal
        pure
        returns (RewardsUpdate memory update)
    {
        update.onlyCurrent = true;
        update.onlyCurrentQuantity = amount;
    }

    function MultiTickReward(int24 startTick, uint128 startLiquidity, uint128[] memory quantities)
        internal
        pure
        returns (RewardsUpdate memory)
    {
        return MultiTickReward(startTick, startLiquidity, quantities, 0);
    }

    function MultiTickReward(
        int24 startTick,
        uint128 startLiquidity,
        uint128[] memory quantities,
        uint72 rewardChecksum
    ) internal pure returns (RewardsUpdate memory) {
        return RewardsUpdate({
            onlyCurrent: false,
            onlyCurrentQuantity: 0,
            startTick: startTick,
            startLiquidity: startLiquidity,
            quantities: quantities,
            rewardChecksum: rewardChecksum
        });
    }

    function re(TickReward memory reward) internal pure returns (TickReward[] memory r) {
        r = new TickReward[](1);
        r[0] = reward;
    }

    function re(TickReward memory r1, TickReward memory r2)
        internal
        pure
        returns (TickReward[] memory r)
    {
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
