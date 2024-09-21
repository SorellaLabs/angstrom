// SPDX-License-Identifier: BUSL-1.1
pragma solidity ^0.8.24;

import {PoolId} from "v4-core/src/types/PoolId.sol";
import {PoolRewards, REWARD_GROWTH_SIZE} from "../types/PoolRewards.sol";
import {CalldataReader} from "../types/CalldataReader.sol";

import {TickLib} from "../libraries/TickLib.sol";
import {MixedSignLib} from "../libraries/MixedSignLib.sol";
import {FixedPointMathLib} from "solady/src/utils/FixedPointMathLib.sol";

import {console} from "forge-std/console.sol";
import {LOG_LEVEL, MED_LEVEL, TRACING_LEVEL, DEBUG_LOGS} from "./DevFlags.sol";
import {FormatLib} from "super-sol/libraries/FormatLib.sol";

/// @author philogy <https://github.com/philogy>
abstract contract RewardsUpdater {
    using FixedPointMathLib for uint256;
    using TickLib for uint256;
    using FormatLib for *;

    error WrongEndLiquidity(uint128 endLiquidity, uint128 actualCurrentLiquidity);

    function _decodeAndReward(CalldataReader reader, PoolRewards storage poolRewards, PoolId id)
        internal
        returns (CalldataReader, uint256 total)
    {
        if (DEBUG_LOGS) {
            console.log(
                "[RewardsUpdater] Entering _decodeAndReward(reader, poolRewards, %x)", uint256(PoolId.unwrap(id))
            );
        }
        uint256 cumulativeGrowth;
        uint128 endLiquidity;
        {
            if (TRACING_LEVEL >= LOG_LEVEL) console.log("[RewardsUpdater] Decoding below flag");
            bool below;
            {
                uint8 rewardUpdateVariantMap;
                (reader, rewardUpdateVariantMap) = reader.readU8();
                below = rewardUpdateVariantMap & 1 != 0;
            }
            if (TRACING_LEVEL >= LOG_LEVEL) console.log("[RewardsUpdater] Decoding startTick");
            int24 startTick;
            (reader, startTick) = reader.readI24();
            if (TRACING_LEVEL >= LOG_LEVEL) {
                console.log("[RewardsUpdater] Retrieving pool %x current tick", uint256(PoolId.unwrap(id)));
            }
            int24 currentTick = _getCurrentTick(id);
            if (TRACING_LEVEL >= LOG_LEVEL) {
                console.log("[RewardsUpdater] Current tick: %s", currentTick.toStr());
                console.log("[RewardsUpdater] Decoding update start liquidity");
            }
            uint128 liquidity;
            (reader, liquidity) = reader.readU128();
            if (TRACING_LEVEL >= LOG_LEVEL) console.log("[RewardsUpdater] Decoding bounds of reward amounts list");
            CalldataReader amountsEnd;
            (reader, amountsEnd) = reader.readU24End();
            if (TRACING_LEVEL >= LOG_LEVEL) console.log("[RewardsUpdater] Starting core reward loop");

            if (MED_LEVEL >= LOG_LEVEL) {
                console.log("[RewardsUpdater] Entering %s rewards method", below ? "below" : "above");
                console.log("  currentTick: %s", currentTick.toStr());
                console.log("  startTick: %s", startTick.toStr());
                console.log("  startLiquidity: %s", liquidity.fmtD(9, 21));
                uint256 d = amountsEnd.offset() - reader.offset();
                console.log("  total amounts: %s (r: %s)", d / 16, d % 16);
                CalldataReader amountReader = reader;
                while (amountReader < amountsEnd) {
                    uint128 nextAmount;
                    (amountReader, nextAmount) = amountReader.readU128();
                    console.log("    amount: %s", nextAmount);
                }
            }

            (reader, total, cumulativeGrowth, endLiquidity) = below
                ? _rewardBelow(poolRewards.rewardGrowthOutside, currentTick, reader, startTick, id, liquidity)
                : _rewardAbove(poolRewards.rewardGrowthOutside, currentTick, reader, startTick, id, liquidity);

            uint128 donateToCurrent;
            (reader, donateToCurrent) = reader.readU128();
            cumulativeGrowth += flatDivWad(donateToCurrent, endLiquidity);
            total += donateToCurrent;

            reader.requireAtEndOf(amountsEnd, "_decodeAndReward:amounts");
        }

        if (TRACING_LEVEL >= LOG_LEVEL) {
            console.log("[RewardsUpdater] Completed core reward loop, checking end liquidity");
        }

        uint128 currentLiquidity = _getCurrentLiquidity(id);
        if (endLiquidity != currentLiquidity) revert WrongEndLiquidity(endLiquidity, currentLiquidity);

        if (DEBUG_LOGS) console.log("[RewardsUpdater] Updating global growth by cumulativeGrowth");

        poolRewards.globalGrowth += cumulativeGrowth;

        return (reader, total);
    }

    function _rewardBelow(
        uint256[REWARD_GROWTH_SIZE] storage rewardGrowthOutside,
        int24 currentTick,
        CalldataReader reader,
        int24 rewardTick,
        PoolId id,
        uint128 liquidity
    ) internal returns (CalldataReader, uint256, uint256, uint128) {
        if (DEBUG_LOGS) console.log("[RewardsUpdater] _rewardBelow");

        bool initialized = true;
        uint256 total = 0;
        uint256 cumulativeGrowth = 0;

        while (rewardTick <= currentTick) {
            if (initialized) {
                if (DEBUG_LOGS) console.log("  %s initialized", rewardTick.toStr());
                uint256 amount;
                (reader, amount) = reader.readU128();
                total += amount;
                if (DEBUG_LOGS) console.log("  added %s (total: %s)", amount.fmtD(9), total.fmtD(9));

                cumulativeGrowth += flatDivWad(amount, liquidity);
                if (DEBUG_LOGS) console.log("  cum. growth: %s", cumulativeGrowth.fmtD(12));

                rewardGrowthOutside[uint24(rewardTick)] += cumulativeGrowth;
                if (DEBUG_LOGS) console.log("  updated growth outside");

                {
                    int128 netLiquidity = _getNetTickLiquidity(id, rewardTick);
                    if (DEBUG_LOGS) console.log("  net liquidity: %s", netLiquidity.fmtD(12, 21));
                    liquidity = MixedSignLib.add(liquidity, netLiquidity);
                }
            } else if (rewardTick > currentTick) {
                if (DEBUG_LOGS) console.log("  %s *not* initialized, breaking", rewardTick.toStr());
                break;
            }

            (initialized, rewardTick) = _findNextTickUp(id, rewardTick);
        }

        if (DEBUG_LOGS) {
            console.log(
                "  reward loop complete. (total: %s, cumulativeGrowth: %s)", total.fmtD(18), cumulativeGrowth.fmtD(18)
            );
        }

        return (reader, total, cumulativeGrowth, liquidity);
    }

    function _rewardAbove(
        uint256[REWARD_GROWTH_SIZE] storage rewardGrowthOutside,
        int24 currentTick,
        CalldataReader reader,
        int24 rewardTick,
        PoolId id,
        uint128 liquidity
    ) internal returns (CalldataReader, uint256, uint256, uint128) {
        if (DEBUG_LOGS) console.log("[RewardsUpdater] _rewardAbove");

        bool initialized = true;
        uint256 total = 0;
        uint256 cumulativeGrowth = 0;

        while (rewardTick > currentTick) {
            if (initialized) {
                if (DEBUG_LOGS) console.log("  %s initialized", rewardTick.toStr());
                uint256 amount;
                (reader, amount) = reader.readU128();
                total += amount;
                if (DEBUG_LOGS) console.log("  added %s (total: %s)", amount.fmtD(9), total.fmtD(9));

                cumulativeGrowth += flatDivWad(amount, liquidity);
                if (DEBUG_LOGS) console.log("  cum. growth: %s", cumulativeGrowth.fmtD(12));

                rewardGrowthOutside[uint24(rewardTick)] += cumulativeGrowth;
                if (DEBUG_LOGS) console.log("  updated growth outside");

                liquidity = MixedSignLib.sub(liquidity, _getNetTickLiquidity(id, rewardTick));
            }
            (initialized, rewardTick) = _findNextTickDown(id, rewardTick);
        }

        if (DEBUG_LOGS) {
            console.log(
                "  reward loop complete. (total: %s, cumulativeGrowth: %s)", total.fmtD(18), cumulativeGrowth.fmtD(18)
            );
        }

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
