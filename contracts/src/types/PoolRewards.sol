// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import {IPoolManager} from "v4-core/src/interfaces/IPoolManager.sol";
import {PoolId} from "v4-core/src/types/PoolId.sol";
import {IUniV4} from "../interfaces/IUniV4.sol";
import {TickLib} from "../libraries/TickLib.sol";

import {console} from "forge-std/console.sol";
import {FormatLib} from "super-sol/libraries/FormatLib.sol";

/// @dev Should accomodate all possible tick values.
uint256 constant REWARD_GROWTH_SIZE = 16777216;

struct PoolRewards {
    uint256[REWARD_GROWTH_SIZE] rewardGrowthOutside;
    uint256 globalGrowth;
}

using PoolRewardsLib for PoolRewards global;

/// @author philogy <https://github.com/philogy>
library PoolRewardsLib {
    using FormatLib for *;

    using IUniV4 for IPoolManager;
    using TickLib for uint256;
    using TickLib for int24;

    function getGrowthInside(PoolRewards storage self, int24 current, int24 lower, int24 upper)
        internal
        view
        returns (uint256)
    {
        uint256 lowerGrowth = self.rewardGrowthOutside[uint24(lower)];
        uint256 upperGrowth = self.rewardGrowthOutside[uint24(upper)];

        if (current < lower) {
            return lowerGrowth - upperGrowth;
        } else if (current >= upper) {
            return upperGrowth - lowerGrowth;
        } else {
            return self.globalGrowth - lowerGrowth - upperGrowth;
        }
    }

    function updateAfterTickMove(PoolRewards storage self, PoolId id, IPoolManager uniV4, int24 prevTick, int24 newTick)
        internal
    {
        prevTick = prevTick.normalize();
        newTick = newTick.normalize();
        if (newTick > prevTick) {
            _updateTickMoveUp(self, uniV4, id, prevTick, newTick);
        } else if (newTick < prevTick) {
            _updateTickMoveDown(self, uniV4, id, prevTick, newTick);
        }
    }

    function _updateTickMoveUp(PoolRewards storage self, IPoolManager uniV4, PoolId id, int24 tick, int24 newTick)
        private
    {
        uint256 globalGrowth = self.globalGrowth;
        do {
            bool initialized;
            (initialized, tick) = uniV4.getNextTickGt(id, tick);

            if (tick <= newTick && initialized) {
                self.rewardGrowthOutside[uint24(tick)] = globalGrowth - self.rewardGrowthOutside[uint24(tick)];
            }
        } while (tick < newTick);
    }

    function _updateTickMoveDown(PoolRewards storage self, IPoolManager uniV4, PoolId id, int24 tick, int24 newTick)
        private
    {
        uint256 globalGrowth = self.globalGrowth;

        while (true) {
            bool initialized;
            (initialized, tick) = uniV4.getNextTickLe(id, tick);

            if (newTick < tick) {
                if (initialized) {
                    self.rewardGrowthOutside[uint24(tick)] = globalGrowth - self.rewardGrowthOutside[uint24(tick)];
                }
            } else {
                break;
            }
            tick--;
        }
    }
}
