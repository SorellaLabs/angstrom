// SPDX-License-Identifier: BUSL-1.1
pragma solidity ^0.8.24;

import {PoolId} from "v4-core/src/types/PoolId.sol";
import {PoolRewards, REWARD_GROWTH_SIZE} from "../types/PoolRewards.sol";
import {CalldataReader} from "../types/CalldataReader.sol";
import {IPoolManager, IUniV4} from "../interfaces/IUniV4.sol";
import {UniConsumer} from "./UniConsumer.sol";

import {TickLib} from "../libraries/TickLib.sol";
import {MixedSignLib} from "../libraries/MixedSignLib.sol";
import {FixedPointMathLib} from "solady/src/utils/FixedPointMathLib.sol";

/// @author philogy <https://github.com/philogy>
/// @dev Core logic responsible for updating reward accumulators to distribute rewards.
abstract contract GrowthOutsideUpdater is UniConsumer {
    using IUniV4 for IPoolManager;
    using FixedPointMathLib for uint256;
    using TickLib for uint256;

    error WrongEndLiquidity(uint128 endLiquidity, uint128 actualCurrentLiquidity);

    // Stack too deep shenanigan.
    struct RewardParams {
        PoolId id;
        int24 tickSpacing;
        int24 currentTick;
    }

    function _decodeAndReward(
        bool currentOnly,
        CalldataReader reader,
        PoolRewards storage poolRewards_,
        PoolId id,
        int24 tickSpacing,
        int24 currentTick
    ) internal returns (CalldataReader, uint256) {
        if (currentOnly) {
            uint128 amount;
            (reader, amount) = reader.readU128();
            poolRewards_.globalGrowth += flatDivWad(amount, UNI_V4.getPoolLiquidity(id));

            return (reader, amount);
        }

        uint256 cumulativeGrowth;
        uint128 endLiquidity;

        int24 startTick;
        (reader, startTick) = reader.readI24();
        uint128 liquidity;
        (reader, liquidity) = reader.readU128();
        (CalldataReader newReader, CalldataReader amountsEnd) = reader.readU24End();

        // Stack too deep shenanigan.
        PoolRewards storage poolRewards = poolRewards_;

        uint256 total;
        RewardParams memory pool = RewardParams(id, tickSpacing, currentTick);
        (newReader, total, cumulativeGrowth, endLiquidity) = startTick <= pool.currentTick
            ? _rewardBelow(poolRewards.rewardGrowthOutside, startTick, newReader, liquidity, pool)
            : _rewardAbove(poolRewards.rewardGrowthOutside, startTick, newReader, liquidity, pool);

        uint128 donateToCurrent;
        (newReader, donateToCurrent) = newReader.readU128();
        cumulativeGrowth += flatDivWad(donateToCurrent, endLiquidity);
        total += donateToCurrent;

        newReader.requireAtEndOf(amountsEnd);

        uint128 currentLiquidity = UNI_V4.getPoolLiquidity(pool.id);
        if (endLiquidity != currentLiquidity) {
            revert WrongEndLiquidity(endLiquidity, currentLiquidity);
        }

        poolRewards.globalGrowth += cumulativeGrowth;

        return (newReader, total);
    }

    function _rewardBelow(
        uint256[REWARD_GROWTH_SIZE] storage rewardGrowthOutside,
        int24 rewardTick,
        CalldataReader reader,
        uint128 liquidity,
        RewardParams memory pool
    ) internal returns (CalldataReader, uint256, uint256, uint128) {
        bool initialized = true;
        uint256 total = 0;
        uint256 cumulativeGrowth = 0;

        do {
            if (initialized) {
                uint256 amount;
                (reader, amount) = reader.readU128();

                total += amount;
                cumulativeGrowth += flatDivWad(amount, liquidity);
                rewardGrowthOutside[uint24(rewardTick)] += cumulativeGrowth;

                (, int128 netLiquidity) = UNI_V4.getTickLiquidity(pool.id, rewardTick);
                liquidity = MixedSignLib.add(liquidity, netLiquidity);
            }
            (initialized, rewardTick) = UNI_V4.getNextTickGt(pool.id, rewardTick, pool.tickSpacing);
        } while (rewardTick <= pool.currentTick);

        return (reader, total, cumulativeGrowth, liquidity);
    }

    function _rewardAbove(
        uint256[REWARD_GROWTH_SIZE] storage rewardGrowthOutside,
        int24 rewardTick,
        CalldataReader reader,
        uint128 liquidity,
        RewardParams memory pool
    ) internal returns (CalldataReader, uint256, uint256, uint128) {
        bool initialized = true;
        uint256 total = 0;
        uint256 cumulativeGrowth = 0;

        do {
            if (initialized) {
                uint256 amount;
                (reader, amount) = reader.readU128();

                total += amount;
                cumulativeGrowth += flatDivWad(amount, liquidity);
                rewardGrowthOutside[uint24(rewardTick)] += cumulativeGrowth;

                (, int128 netLiquidity) = UNI_V4.getTickLiquidity(pool.id, rewardTick);
                liquidity = MixedSignLib.sub(liquidity, netLiquidity);
            }
            (initialized, rewardTick) = UNI_V4.getNextTickLt(pool.id, rewardTick, pool.tickSpacing);
        } while (rewardTick > pool.currentTick);

        return (reader, total, cumulativeGrowth, liquidity);
    }

    /**
     * @dev Overflow-safe fixed-point division of `x / y` resulting in `0` if `y` is zero.
     */
    function flatDivWad(uint256 x, uint256 y) internal pure returns (uint256) {
        return (x * FixedPointMathLib.WAD).rawDiv(y);
    }
}
