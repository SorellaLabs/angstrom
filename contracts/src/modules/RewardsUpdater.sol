// SPDX-License-Identifier: UNLICENSED
pragma solidity ^0.8.24;

import {PoolId} from "v4-core/src/types/PoolId.sol";
import {PoolRewards} from "../types/PoolRewards.sol";
import {CalldataReader} from "../types/CalldataReader.sol";

import {TickLib} from "../libraries/TickLib.sol";
import {MixedSignLib} from "../libraries/MixedSignLib.sol";
import {FixedPointMathLib} from "solady/src/utils/FixedPointMathLib.sol";

import {console} from "forge-std/console.sol";
import {DEBUG_LOGS} from "./DevFlags.sol";
import {FormatLib} from "super-sol/libraries/FormatLib.sol";

/// @author philogy <https://github.com/philogy>
abstract contract RewardsUpdater {
    using FixedPointMathLib for uint256;
    using TickLib for uint256;
    using FormatLib for *;

    error WrongEndLiquidity(uint128, uint128);

    function _decodeAndReward(PoolRewards storage poolRewards, PoolId id, CalldataReader reader)
        internal
        returns (CalldataReader, uint256 total)
    {
        if (DEBUG_LOGS) console.log("[RewardsUpdater] Entering _decodeAndReward");
        uint256 cumulativeGrowth;
        uint128 endLiquidity;
        {
            if (DEBUG_LOGS) console.log("[RewardsUpdater] Decoding startTick");
            int24 startTick;
            (reader, startTick) = reader.readI24();
            if (DEBUG_LOGS) console.log("[RewardsUpdater] Retrieving pool %x current tick", uint256(PoolId.unwrap(id)));
            int24 currentTick = _getCurrentTick(id);
            if (DEBUG_LOGS) console.log("[RewardsUpdater] Decoding update start liquidity");
            uint128 liquidity;
            (reader, liquidity) = reader.readU128();
            if (DEBUG_LOGS) console.log("[RewardsUpdater] Decoding bounds of reward amounts list");
            CalldataReader amountsEnd;
            (reader, amountsEnd) = reader.readU24End();
            if (DEBUG_LOGS) console.log("[RewardsUpdater] Starting core reward loop");
            (reader, total, cumulativeGrowth, endLiquidity) = startTick <= currentTick
                ? _rewardBelow(poolRewards.rewardGrowthOutside, currentTick, reader, startTick, id, liquidity, amountsEnd)
                : _rewardAbove(poolRewards.rewardGrowthOutside, currentTick, reader, startTick, id, liquidity, amountsEnd);
        }

        if (DEBUG_LOGS) console.log("[RewardsUpdater] Completed core reward loop, checking end liquidity");

        uint128 currentLiquidity = _getCurrentLiquidity(id);
        if (endLiquidity != currentLiquidity) revert WrongEndLiquidity(endLiquidity, currentLiquidity);

        if (DEBUG_LOGS) console.log("[RewardsUpdater] Updating global growth by cumulativeGrowth");

        poolRewards.globalGrowth += cumulativeGrowth;

        return (reader, total);
    }

    function _rewardBelow(
        mapping(int24 => uint256) storage rewardGrowthOutside,
        int24 endTick,
        CalldataReader reader,
        int24 tick,
        PoolId id,
        uint128 liquidity,
        CalldataReader amountsEnd
    ) internal returns (CalldataReader, uint256, uint256, uint128) {
        if (DEBUG_LOGS) console.log("[RewardsUpdater] entering _rewardBelow");

        // To not waste a loop iteration we assume the given tick is a valid initialized tick.
        // TODO(security): Check that this doesn't allow breaking invariants if invoked with an
        // unitialized tick.
        bool initialized = true;
        uint256 total = 0;
        uint256 cumulativeGrowth = 0;

        if (DEBUG_LOGS) console.log("[RewardsUpdater] total: %s", total);

        do {
            if (DEBUG_LOGS) {
                console.log(
                    "[RewardsUpdater] reward update loop (initialized: %s, tick: %s, liquidity: %s)",
                    initialized.toStr(),
                    tick.toStr(),
                    liquidity
                );
            }
            if (initialized) {
                if (DEBUG_LOGS) console.log("[RewardsUpdater] Initialized, updating tick");
                // Amounts beyond the end of the sequence default to 0.
                uint256 amount = 0;
                if (reader != amountsEnd) (reader, amount) = reader.readU128();
                total += amount;

                cumulativeGrowth += flatDivWad(amount, liquidity);
                rewardGrowthOutside[tick] += cumulativeGrowth;

                liquidity = MixedSignLib.add(liquidity, _getNetTickLiquidity(id, tick));

                if (DEBUG_LOGS) {
                    console.log("[RewardsUpdater] Adding %s to tick %s", amount, tick.toStr());
                    console.log("[RewardsUpdater] New total: %s", total);
                    console.log(
                        "[RewardsUpdater] Increasing tick %s growth outside by %e18", tick.toStr(), cumulativeGrowth
                    );
                    console.log("[RewardsUpdater] Retrieved and updated liquidity to: %s", liquidity);
                }
            }

            (initialized, tick) = _findNextTickUp(id, tick);

            // Break condition is the current tick bound to account for situations where the
            // "current tick" is uninitialized.
        } while (tick <= endTick);

        if (DEBUG_LOGS) console.log("[RewardsUpdater] Main reward loop complete.");

        if (reader != amountsEnd) {
            if (DEBUG_LOGS) console.log("[RewardsUpdater] Reading additional amount for current tick");
            uint256 currentTickReward;
            (reader, currentTickReward) = reader.readU128();
            if (DEBUG_LOGS) console.log("[RewardsUpdater] currentTickReward: %s", currentTickReward);
            total += currentTickReward;
            cumulativeGrowth += flatDivWad(currentTickReward, liquidity);
        }

        if (DEBUG_LOGS) {
            console.log("[RewardsUpdater] Final values (total: %s, cumulativeGrowth: s)", total, cumulativeGrowth);
        }

        reader.requireAtEndOf(amountsEnd);

        return (reader, total, cumulativeGrowth, liquidity);
    }

    function _rewardAbove(
        mapping(int24 => uint256) storage rewardGrowthOutside,
        int24 endTick,
        CalldataReader reader,
        int24 tick,
        PoolId id,
        uint128 liquidity,
        CalldataReader amountsEnd
    ) internal returns (CalldataReader, uint256, uint256, uint128) {
        if (DEBUG_LOGS) console.log("[RewardsUpdater] entering _rewardAbove");

        // To not waste a loop iteration we assume the given tick is a valid initialized tick.
        // TODO(security): Check that this doesn't allow breaking invariants if invoked with an
        // unitialized tick.
        bool initialized = true;
        uint256 total = 0;
        uint256 cumulativeGrowth = 0;

        do {
            if (initialized) {
                if (DEBUG_LOGS) console.log("[RewardsUpdater] Initialized, updating tick");
                // Amounts beyond the end of the sequence default to 0.
                uint256 amount = 0;
                if (reader != amountsEnd) (reader, amount) = reader.readU128();
                total += amount;

                cumulativeGrowth += flatDivWad(amount, liquidity);
                rewardGrowthOutside[tick] += cumulativeGrowth;

                liquidity = MixedSignLib.sub(liquidity, _getNetTickLiquidity(id, tick));

                if (DEBUG_LOGS) {
                    console.log("[RewardsUpdater] Adding %s to tick %s", amount, tick.toStr());
                    console.log("[RewardsUpdater] New total: %s", total);
                    console.log(
                        "[RewardsUpdater] Increasing tick %s growth outside by %e18", tick.toStr(), cumulativeGrowth
                    );
                    console.log("[RewardsUpdater] Retrieved and updated liquidity to: %s", liquidity);
                }
            }

            (initialized, tick) = _findNextTickDown(id, tick);

            // Break condition is the current tick bound to account for situations where the
            // "current tick" is uninitialized.
        } while (endTick < tick);

        if (DEBUG_LOGS) console.log("[RewardsUpdater] Main reward loop complete.");

        if (reader != amountsEnd) {
            if (DEBUG_LOGS) console.log("[RewardsUpdater] Reading additional amount for current tick");
            uint256 currentTickReward;
            (reader, currentTickReward) = reader.readU128();
            if (DEBUG_LOGS) console.log("[RewardsUpdater] currentTickReward: %s", currentTickReward);
            total += currentTickReward;
            cumulativeGrowth += flatDivWad(currentTickReward, liquidity);
        }

        if (DEBUG_LOGS) {
            console.log("[RewardsUpdater] Final values (total: %s, cumulativeGrowth: s)", total, cumulativeGrowth);
        }

        reader.requireAtEndOf(amountsEnd);

        return (reader, total, cumulativeGrowth, liquidity);
    }

    function _findNextTickUp(PoolId id, int24 tick) internal view returns (bool initialized, int24 newTick) {
        (int16 wordPos, uint8 bitPos) = TickLib.position(TickLib.compress(tick) + 1);
        (initialized, bitPos) = _getPoolBitmapInfo(id, wordPos).nextBitPosGte(bitPos);
        newTick = TickLib.toTick(wordPos, bitPos);
    }

    function _findNextTickDown(PoolId id, int24 tick) internal view returns (bool initialized, int24 newTick) {
        (int16 wordPos, uint8 bitPos) = TickLib.position(TickLib.compress(tick - 1));
        (initialized, bitPos) = _getPoolBitmapInfo(id, wordPos).nextBitPosLte(bitPos);
        newTick = TickLib.toTick(wordPos, bitPos);
    }

    /**
     * @dev Overflow-safe fixed-point division of `x / y` resulting in `0` if `y` is zero.
     */
    function flatDivWad(uint256 x, uint256 y) internal pure returns (uint256) {
        return (x * FixedPointMathLib.WAD).rawDiv(y);
    }

    ////////////////////////////////////////////////////////////////
    //                       POOL INTERFACE                       //
    ////////////////////////////////////////////////////////////////

    function _getPoolBitmapInfo(PoolId id, int16 wordPos) internal view virtual returns (uint256);
    function _getNetTickLiquidity(PoolId id, int24 tick) internal view virtual returns (int128);
    function _getCurrentLiquidity(PoolId id) internal view virtual returns (uint128);
    function _getCurrentTick(PoolId id) internal view virtual returns (int24);
}
