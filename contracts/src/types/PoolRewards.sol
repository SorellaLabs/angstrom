// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import {IPoolManager} from "v4-core/src/interfaces/IPoolManager.sol";
import {PoolId} from "v4-core/src/types/PoolId.sol";
import {IUniV4} from "../interfaces/IUniV4.sol";
import {TickLib} from "../libraries/TickLib.sol";

/// @dev Should accomodate all possible tick values.
uint256 constant REWARD_GROWTH_SIZE = 16777216;

struct PoolRewards {
    uint256[REWARD_GROWTH_SIZE] rewardGrowthOutside;
    uint256 globalGrowth;
}

using PoolRewardsLib for PoolRewards global;

/// @author philogy <https://github.com/philogy>
/// @dev Computes and maintains global LP rewards.
library PoolRewardsLib {
    using IUniV4 for IPoolManager;
    using TickLib for uint256;
    using TickLib for int24;

    function getGrowthInside(PoolRewards storage self, int24 current, int24 lower, int24 upper)
        internal
        view
        returns (uint256 growthInside)
    {
        uint256 lowerGrowth = self.rewardGrowthOutside[uint24(lower)];
        uint256 upperGrowth = self.rewardGrowthOutside[uint24(upper)];

        if (current < lower) {
            return lowerGrowth - upperGrowth;
        }
        if (upper <= current) {
            return upperGrowth - lowerGrowth;
        }
        return growthInside = self.globalGrowth - lowerGrowth - upperGrowth;
    }

    /// @dev Update growth values for a valid tick move from `prevTick` to `newTick`. Expects
    /// `prevTick` and `newTick` to be valid Uniswap ticks (defined as tick ∈ [TickMath.MIN_TICK;
    /// TickMath.MAX_TICK]).
    function updateAfterTickMove(
        PoolRewards storage self,
        PoolId id,
        IPoolManager uniV4,
        int24 prevTick,
        int24 newTick,
        int24 tickSpacing
    ) internal {
        if (newTick > prevTick) {
            // We assume the ticks are valid so no risk of underflow with these calls.
            if (newTick.normalizeUnchecked(tickSpacing) > prevTick) {
                _updateTickMoveUp(self, uniV4, id, prevTick, newTick, tickSpacing);
            }
        } else if (newTick < prevTick) {
            // We assume the ticks are valid so no risk of underflow with these calls.
            if (newTick < prevTick.normalizeUnchecked(tickSpacing)) {
                _updateTickMoveDown(self, uniV4, id, prevTick, newTick, tickSpacing);
            }
        }
    }

    function _updateTickMoveUp(
        PoolRewards storage self,
        IPoolManager uniV4,
        PoolId id,
        int24 tick,
        int24 newTick,
        int24 tickSpacing
    ) private {
        while (true) {
            bool initialized;
            (initialized, tick) = uniV4.getNextTickGt(id, tick, tickSpacing);

            if (newTick < tick) break;
            if (initialized) {
                self.rewardGrowthOutside[uint24(tick)] =
                    self.globalGrowth - self.rewardGrowthOutside[uint24(tick)];
            }
        }
    }

    function _updateTickMoveDown(
        PoolRewards storage self,
        IPoolManager uniV4,
        PoolId id,
        int24 tick,
        int24 newTick,
        int24 tickSpacing
    ) private {
        while (true) {
            bool initialized;
            (initialized, tick) = uniV4.getNextTickLe(id, tick, tickSpacing);

            if (tick <= newTick) break;

            if (initialized) {
                self.rewardGrowthOutside[uint24(tick)] =
                    self.globalGrowth - self.rewardGrowthOutside[uint24(tick)];
            }
            tick--;
        }
    }
}
