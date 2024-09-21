// SPDX-License-Identifier: BUSL-1.1
pragma solidity ^0.8.24;

import {PoolId} from "v4-core/src/types/PoolId.sol";
import {PoolRewards, REWARD_GROWTH_SIZE} from "../types/PoolRewards.sol";
import {CalldataReader} from "../types/CalldataReader.sol";

import {TickLib} from "../libraries/TickLib.sol";
import {MixedSignLib} from "../libraries/MixedSignLib.sol";
import {FixedPointMathLib} from "solady/src/utils/FixedPointMathLib.sol";

import {console} from "forge-std/console.sol";
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
        uint256 cumulativeGrowth;
        uint128 endLiquidity;
        {
            bool below;
            {
                uint8 rewardUpdateVariantMap;
                (reader, rewardUpdateVariantMap) = reader.readU8();
                below = rewardUpdateVariantMap & 1 != 0;
            }
            int24 startTick;
            (reader, startTick) = reader.readI24();
            int24 currentTick = _getCurrentTick(id);
            uint128 liquidity;
            (reader, liquidity) = reader.readU128();
            CalldataReader amountsEnd;
            (reader, amountsEnd) = reader.readU24End();

            (reader, total, cumulativeGrowth, endLiquidity) = below
                ? _rewardBelow(poolRewards.rewardGrowthOutside, currentTick, reader, startTick, id, liquidity)
                : _rewardAbove(poolRewards.rewardGrowthOutside, currentTick, reader, startTick, id, liquidity);

            uint128 donateToCurrent;
            (reader, donateToCurrent) = reader.readU128();
            cumulativeGrowth += flatDivWad(donateToCurrent, endLiquidity);
            total += donateToCurrent;

            reader.requireAtEndOf(amountsEnd);
        }

        uint128 currentLiquidity = _getCurrentLiquidity(id);
        if (endLiquidity != currentLiquidity) revert WrongEndLiquidity(endLiquidity, currentLiquidity);

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
        bool initialized = true;
        uint256 total = 0;
        uint256 cumulativeGrowth = 0;

        while (rewardTick <= currentTick) {
            if (initialized) {
                uint256 amount;
                (reader, amount) = reader.readU128();

                total += amount;
                cumulativeGrowth += flatDivWad(amount, liquidity);
                rewardGrowthOutside[uint24(rewardTick)] += cumulativeGrowth;

                liquidity = MixedSignLib.add(liquidity, _getNetTickLiquidity(id, rewardTick));
            }
            (initialized, rewardTick) = _findNextTickUp(id, rewardTick);
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
        bool initialized = true;
        uint256 total = 0;
        uint256 cumulativeGrowth = 0;

        while (rewardTick > currentTick) {
            if (initialized) {
                uint256 amount;
                (reader, amount) = reader.readU128();

                total += amount;
                cumulativeGrowth += flatDivWad(amount, liquidity);
                rewardGrowthOutside[uint24(rewardTick)] += cumulativeGrowth;

                liquidity = MixedSignLib.sub(liquidity, _getNetTickLiquidity(id, rewardTick));
            }
            (initialized, rewardTick) = _findNextTickDown(id, rewardTick);
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
