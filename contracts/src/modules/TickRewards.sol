// SPDX-License-Identifier: MIT
pragma solidity ^0.8.4;

struct TickRewards {
    mapping(int24 tick => uint256 cumulativeFeeGrowth) growthOutside;
    uint256 globalGrowthOutside;
}

using TickRewardsLib for TickRewards global;

library TickRewardsLib {
    function getGrowthInside(TickRewards storage self, int24 current, int24 lower, int24 upper)
        internal
        view
        returns (uint256)
    {
        uint256 lowerGrowth = self.growthOutside[lower];
        uint256 upperGrowth = self.growthOutside[upper];
        if (current < lower) {
            return lowerGrowth - upperGrowth;
        } else if (current >= upper) {
            return upperGrowth - lowerGrowth;
        } else {
            return self.globalGrowthOutside - lowerGrowth - upperGrowth;
        }
    }
}
