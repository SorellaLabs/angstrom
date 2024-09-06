// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "../_helpers/BaseTest.sol";
import {RewardsUpdater, PoolRewards} from "../../src/modules/RewardsUpdater.sol";
import {PoolRewards} from "../../src/types/PoolRewards.sol";
import {ConversionLib} from "../../src/libraries/ConversionLib.sol";
import {CalldataReader, CalldataReaderLib} from "src/types/CalldataReader.sol";
import {SafeCastLib} from "solady/src/utils/SafeCastLib.sol";

import {PoolId, PoolIdLibrary} from "v4-core/src/types/PoolId.sol";
import {RewardsUpdate} from "../../src/reference/PoolRewardsUpdate.sol";
import {TICK_SPACING} from "../../src/Constants.sol";
import {UniV4Inspector} from "../_view-ext/UniV4Inspector.sol";
import {MockERC20} from "super-sol/mocks/MockERC20.sol";
import {TickMath} from "v4-core/src/libraries/TickMath.sol";
import {TickRangeMap} from "../_helpers/TickRangeMap.sol";

import {PoolGate} from "../_helpers/PoolGate.sol";
import {FixedPointMathLib} from "solady/src/utils/FixedPointMathLib.sol";
import {SafeCastLib} from "solady/src/utils/SafeCastLib.sol";

import {PRNG} from "super-sol/collections/PRNG.sol";
import {FormatLib} from "super-sol/libraries/FormatLib.sol";
import {console2 as console} from "forge-std/console2.sol";

/// @author philogy <https://github.com/philogy>
contract RewardsUpdaterTest is BaseTest, RewardsUpdater {
    using TickMath for int24;

    using FormatLib for *;
    using FixedPointMathLib for *;
    using SafeCastLib for *;

    uint160 constant ONE_SQRTX96 = 1 << 96;

    address owner = makeAddr("owner");
    UniV4Inspector uniV4;
    PoolGate gate;
    address asset0 = address(new MockERC20());
    address asset1 = address(new MockERC20());

    PoolRewards mainRewards;
    PoolId mainId;

    function setUp() public {
        if (asset1 < asset0) (asset0, asset1) = (asset1, asset0);
        mainId = PoolIdLibrary.toId(ConversionLib.toPoolKey(address(0), asset0, asset1));

        vm.prank(owner);
        uniV4 = new UniV4Inspector();
        gate = new PoolGate(address(uniV4));
    }

    struct Position {
        uint128 liq;
        int24 lowerTick;
        int24 upperTick;
    }

    struct Vars {
        // Start parameters.
        int24 poolTick;
        int8 ticksOff;
        int24 minTick;
        Position[] positions;
        // In-memory pool state.
        TickRangeMap map;
        uint256[] totalLiqAtIndex;
        bool[] inits;
        // Donate params
        uint256 donateTicks;
        uint128[] amounts;
        // Donate verification parameters.
        uint256 total;
        uint256 realTotal;
    }

    function test_reward(int256 input_startCompressedTick, uint256 seed) public {
        Vars memory v;
        PRNG memory rng = PRNG(seed);

        // Setup pool.
        v.poolTick = int24(bound(input_startCompressedTick, -2500, 2500)) * TICK_SPACING;
        gate.initializePool(asset0, asset1, v.poolTick.getSqrtPriceAtTick());

        // Init base params
        v.ticksOff = int8(rng.randbool() ? rng.randint(-64, -1) : rng.randint(1, 64));
        v.minTick = v.ticksOff < 0 ? v.poolTick + TICK_SPACING * v.ticksOff : v.poolTick + TICK_SPACING;

        // Generate random positions and compute total liquidity for later.
        v.positions = new Position[](4);
        v.inits = new bool[](v.ticksOff.abs() + 1);
        v.totalLiqAtIndex = new uint256[](v.inits.length);

        for (uint256 i = 0; i < v.positions.length; i++) {
            Position memory p = v.positions[i];
            // `lo` a bit biased towards the middle
            uint256 lo = rng.randuint(0, v.inits.length - 1);
            uint256 hi = rng.randuint(lo + 1, v.inits.length);
            v.inits[lo] = true;
            v.inits[hi] = true;

            p.liq = u128(rng.randuint(0.1e21, 1.0e21));
            for (uint256 j = lo; j < hi; j++) {
                v.totalLiqAtIndex[j] += p.liq;
            }

            p.lowerTick = _tick(v.minTick, lo);
            p.upperTick = _tick(v.minTick, hi);

            gate.addLiquidity(asset0, asset1, p.lowerTick, p.upperTick, p.liq);
        }

        // Initialize map.
        for (uint256 i = 0; i < v.inits.length; i++) {
            if (v.inits[i]) v.map.add(_tick(v.minTick, i), i);
        }
        assertGe(v.map.size, 2, "Not even one tick was initialized.");

        // Generate random amounts to be donated.
        v.donateTicks = rng.randuint(1, v.map.size);
        v.amounts = new uint128[](v.donateTicks);
        for (uint256 i = 0; i < v.amounts.length; i++) {
            v.amounts[i] = rng.randchoice(0.2e18, 0, rng.randuint(10.0e18)).toUint128();
        }

        // Perform donate
        if (v.ticksOff < 0) {
            // Donating below.

            // Liquidity of actual "start tick" (first tick which will be attributed rewards).
            uint128 startLiquidity = u128(v.totalLiqAtIndex[v.map.brangeToIndex(v.donateTicks)]);
            // Tick from which to start updating total growth outside.
            int24 startTick = v.map.brangeToTick(v.donateTicks - 1);

            v.total = this.__reward(
                RewardsUpdate({startTick: startTick, startLiquidity: startLiquidity, quantities: v.amounts}).encode()
            );
        } else {
            // Donating above.

            // Liquidity of start tick.
            uint128 startLiquidity = u128(v.totalLiqAtIndex[v.map.rangeToIndex(v.donateTicks - 1)]);
            int24 startTick = v.map.rangeToTick(v.donateTicks - 1);

            v.total = this.__reward(
                RewardsUpdate({startTick: startTick, startLiquidity: startLiquidity, quantities: v.amounts}).encode()
            );
        }

        v.realTotal = 0;
        for (uint256 i = 0; i < v.amounts.length; i++) {
            v.realTotal += v.amounts[i];
        }
        assertEq(v.total, v.realTotal, "Donate's total did not match computed total");

        for (uint256 i = 0; i < v.positions.length; i++) {
            Position memory pos = v.positions[i];
            uint256 expectedEarnings = 0;
            for (uint256 j = 0; j < v.donateTicks; j++) {
                int24 tick = v.ticksOff < 0 ? v.map.brangeToTick(v.donateTicks - j) : v.map.rangeToTick(j);
                uint256 index = v.ticksOff < 0 ? v.map.brangeToIndex(v.donateTicks - j) : v.map.rangeToIndex(j);
                if (pos.lowerTick <= tick && tick < pos.upperTick) {
                    uint256 amount = v.ticksOff < 0 ? v.amounts[j] : v.amounts[v.amounts.length - j - 1];
                    expectedEarnings += amount * pos.liq / v.totalLiqAtIndex[index];
                }
            }
            uint256 growth = mainRewards.getGrowthInside(v.poolTick, pos.lowerTick, pos.upperTick).mulWad(pos.liq);
            assertApproxEqRel(expectedEarnings, growth, 1e8, "unexpected earnings");
        }
    }

    function __reward(bytes calldata encodedRewardUpdate) external returns (uint256 total) {
        CalldataReader reader = CalldataReaderLib.from(encodedRewardUpdate);
        (reader, total) = _decodeAndReward(mainRewards, mainId, reader);
        reader.requireAtEndOf(encodedRewardUpdate);
    }

    function _getPoolBitmapInfo(PoolId id, int16 wordPos) internal view override returns (uint256) {
        return uniV4.getBitmapWord(id, wordPos);
    }

    function _getNetTickLiquidity(PoolId id, int24 tick) internal view override returns (int128 liquidityNet) {
        (, liquidityNet,,) = uniV4.getTick(id, tick);
    }

    function _getCurrentLiquidity(PoolId id) internal view override returns (uint128 liquidity) {
        (,,,,,, liquidity) = uniV4.getPool(id);
    }

    function _getCurrentTick(PoolId id) internal view override returns (int24 tick) {
        (, tick,,,,,) = uniV4.getPool(id);
    }

    function _tick(int24 min, uint256 index) internal pure returns (int24) {
        return min + i24(index) * TICK_SPACING;
    }
}
