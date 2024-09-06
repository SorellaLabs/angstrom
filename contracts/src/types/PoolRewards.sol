// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

struct PoolRewards {
    mapping(int24 => uint256) rewardGrowthOutside;
    uint256 globalGrowth;
}

using PoolRewardsLib for PoolRewards global;

/// @author philogy <https://github.com/philogy>
library PoolRewardsLib {
    function getGrowthInside(PoolRewards storage self, int24 current, int24 lower, int24 upper)
        internal
        view
        returns (uint256)
    {
        uint256 lowerGrowth = self.rewardGrowthOutside[lower];
        uint256 upperGrowth = self.rewardGrowthOutside[upper];
        if (current < lower) {
            return lowerGrowth - upperGrowth;
        } else if (current >= upper) {
            return upperGrowth - lowerGrowth;
        } else {
            return self.globalGrowth - lowerGrowth - upperGrowth;
        }
    }
}
