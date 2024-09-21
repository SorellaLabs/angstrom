// SPDX-License-Identifier: BUSL-1.1
pragma solidity ^0.8.0;

import {PoolId} from "v4-core/src/types/PoolId.sol";
import {IPoolManager} from "v4-core/src/interfaces/IPoolManager.sol";
import {IUniV4} from "../../src/interfaces/IUniV4.sol";
import {RewardsUpdate} from "../../src/reference/PoolUpdate.sol";
import {TickLib} from "src/libraries/TickLib.sol";
import {MixedSignLib} from "src/libraries/MixedSignLib.sol";
import {VecLib, UintVec} from "super-sol/collections/Vec.sol";

import {FormatLib} from "super-sol/libraries/FormatLib.sol";

struct TickReward {
    int24 tick;
    uint128 amount;
}

using RewardLib for TickReward global;

/// @author philogy <https://github.com/philogy>
library RewardLib {
    using FormatLib for *;

    using RewardLib for TickReward[];
    using IUniV4 for IPoolManager;
    using TickLib for *;

    function gt(TickReward memory a, TickReward memory b) internal pure returns (bool) {
        return a.tick > b.tick;
    }

    function sort(TickReward[] memory rewards) internal pure {
        for (uint256 i = 0; i < rewards.length; i++) {
            for (uint256 j = i + 1; j < rewards.length; j++) {
                if (rewards[i].gt(rewards[j])) (rewards[i], rewards[j]) = (rewards[j], rewards[i]);
            }
        }
    }

    function findTickGte(TickReward[] memory rewards, int24 tick) internal pure returns (uint256) {
        require(rewards.length > 0, "No rewards");
        for (uint256 i = 0; i < rewards.length; i++) {
            if (rewards[i].tick > tick) return i;
        }
        return rewards.length;
    }

    function toUpdates(TickReward[] memory rewards, IPoolManager uni, PoolId id)
        internal
        view
        returns (RewardsUpdate[] memory updates)
    {
        updates = _toUpdates(rewards, uni, id);
    }

    function _toUpdates(TickReward[] memory rewards, IPoolManager uni, PoolId id)
        internal
        view
        returns (RewardsUpdate[] memory updates)
    {
        if (rewards.length == 0) return updates;

        _checkTicksInitialized(uni, id, rewards);
        _checkSortedUnique(rewards);

        int24 currentTick = uni.getSlot0(id).tick();
        int24 currentTickNormalized = currentTick.normalize();

        // Ensure current tick update doesn't get separated into its own update.
        if (rewards[0].tick == currentTickNormalized) {
            updates = new RewardsUpdate[](1);
            updates[0] = createRewardUpdateAbove(uni, id, rewards, currentTickNormalized);
            return updates;
        } else if (rewards[rewards.length - 1].tick == currentTickNormalized) {
            updates = new RewardsUpdate[](1);
            updates[0] = createRewardUpdateBelow(uni, id, rewards, currentTick);
            return updates;
        }

        uint256 index = rewards.findTickGte(currentTickNormalized);
        TickReward[] memory rewardsBelow = new TickReward[](index);
        TickReward[] memory rewardsAbove = new TickReward[](rewards.length - index);
        for (uint256 i = 0; i < rewards.length; i++) {
            if (i < index) {
                rewardsBelow[i] = rewards[i];
            } else {
                rewardsAbove[i - index] = rewards[i];
            }
        }

        if (rewardsBelow.length == 0) {
            updates = new RewardsUpdate[](1);
            updates[0] = createRewardUpdateAbove(uni, id, rewardsAbove, currentTickNormalized);
            return updates;
        } else if (rewardsAbove.length == 0) {
            updates = new RewardsUpdate[](1);
            updates[0] = createRewardUpdateBelow(uni, id, rewardsBelow, currentTick);
            return updates;
        } else {
            updates = new RewardsUpdate[](2);
            updates[0] = createRewardUpdateAbove(uni, id, rewardsAbove, currentTickNormalized);
            updates[1] = createRewardUpdateBelow(uni, id, rewardsBelow, currentTick);
            return updates;
        }
    }

    function _checkTicksInitialized(IPoolManager uni, PoolId id, TickReward[] memory rewards) private view {
        for (uint256 i = 0; i < rewards.length; i++) {
            int24 tick = rewards[i].tick;
            (int16 wordPos, uint8 bitPos) = TickLib.position(TickLib.compress(tick));
            uint256 word = uni.getPoolBitmapInfo(id, wordPos);
            require(word.isInitialized(bitPos), "Tick not initialized");
        }
    }

    function _checkSortedUnique(TickReward[] memory rewards) private pure {
        rewards.sort();
        {
            int24 lastTick = type(int24).min;
            for (uint256 i = 0; i < rewards.length; i++) {
                int24 tick = rewards[i].tick;
                require(tick > lastTick, "Duplicate tick");
                lastTick = tick;
            }
        }
    }

    function getCurrentTickRangeStart(IPoolManager uni, PoolId id, int24 currentTick) internal view returns (int24) {
        bool initialized;
        while (true) {
            (initialized, currentTick) = uni.getNextTickLe(id, currentTick);
            if (initialized) return currentTick;
        }
        revert("unreachable");
    }

    function createRewardUpdateBelow(IPoolManager uni, PoolId id, TickReward[] memory rewards, int24 currentTick)
        internal
        view
        returns (RewardsUpdate memory update)
    {
        require(rewards.length > 0, "No rewards");

        update.below = true;

        // Create list of initialized ticks, including start (checked before) and the tick of the
        // current range.
        int24 tick = rewards[0].tick;
        bool initialized = true;
        UintVec memory initializedTicks = VecLib.uint_with_cap(rewards.length * 3 / 2);
        while (true) {
            (initialized, tick) = uni.getNextTickGt(id, tick);
            if (currentTick < tick) break;
            if (initialized) initializedTicks.push(uint256(int256(tick)));
        }

        update.quantities = new uint128[](initializedTicks.length + 1);

        uint256 ri = 0;
        int128 cumulativeNetLiquidity = 0;

        for (uint256 i = 0; i < initializedTicks.length; i++) {
            tick = int24(int256(initializedTicks.get(i)));
            int128 tickNetLiquidity;
            (, tickNetLiquidity) = uni.getTickLiquidity(id, tick);
            cumulativeNetLiquidity += tickNetLiquidity;
            if (ri < rewards.length) {
                TickReward memory reward = rewards[ri];
                if (reward.tick < tick) {
                    update.quantities[i] = reward.amount;
                    ri++;
                }
            }
        }

        if (ri < rewards.length) {
            update.quantities[initializedTicks.length] = rewards[ri].amount;
            ri++;
        }

        require(ri == rewards.length, "Not all rewards used?");

        update.startTick = initializedTicks.length > 0 ? int24(uint24(initializedTicks.get(0))) : currentTick + 1;
        uint128 poolLiq = uni.getPoolLiquidity(id);
        update.startLiquidity = MixedSignLib.sub(poolLiq, cumulativeNetLiquidity);
    }

    function createRewardUpdateAbove(IPoolManager uni, PoolId id, TickReward[] memory rewards, int24 currentTick)
        internal
        view
        returns (RewardsUpdate memory update)
    {
        require(rewards.length > 0, "No rewards");

        update.below = false;

        UintVec memory amounts = VecLib.uint_with_cap(rewards.length * 2);

        uint256 lastIndex = rewards.length - 1;
        int24 tick = update.startTick = rewards[lastIndex].tick;
        amounts.push(rewards[lastIndex].amount);
        uint256 i = 1;
        bool initialized;
        int128 cumulativeNetLiquidity = 0;

        if (currentTick < tick) {
            while (true) {
                int128 tickNetLiquidity;
                (, tickNetLiquidity) = uni.getTickLiquidity(id, tick);
                cumulativeNetLiquidity += tickNetLiquidity;

                (initialized, tick) = uni.getNextTickLt(id, tick);

                if (tick <= currentTick) break;

                if (initialized) {
                    if (i < rewards.length) {
                        TickReward memory reward = rewards[rewards.length - i - 1];

                        if (tick <= reward.tick) {
                            amounts.push(reward.amount);
                            i++;
                            continue;
                        }
                    }
                    amounts.push(0);
                }
            }
            if (i < rewards.length) {
                TickReward memory reward = rewards[rewards.length - i - 1];
                amounts.push(reward.amount);
                i++;
            } else {
                amounts.push(0);
            }
        } else {}

        require(i == rewards.length, "Not all / too many amounts used");

        update.startLiquidity = MixedSignLib.add(uni.getPoolLiquidity(id), cumulativeNetLiquidity);
        update.quantities = new uint128[](amounts.length);
        for (i = 0; i < amounts.length; i++) {
            update.quantities[i] = uint128(amounts.get(i));
        }
    }
}
