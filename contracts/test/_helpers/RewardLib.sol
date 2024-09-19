// SPDX-License-Identifier: BUSL-1.1
pragma solidity ^0.8.0;

import {PoolId} from "v4-core/src/types/PoolId.sol";
import {IPoolManager} from "v4-core/src/interfaces/IPoolManager.sol";
import {IUniV4} from "../../src/interfaces/IUniV4.sol";
import {RewardsUpdate} from "../../src/reference/PoolUpdate.sol";
import {TickLib} from "src/libraries/TickLib.sol";
import {MixedSignLib} from "src/libraries/MixedSignLib.sol";
import {VecLib, UintVec} from "super-sol/collections/Vec.sol";

import {console} from "forge-std/console.sol";
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
    using TickLib for uint256;

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
        if (rewards.length == 0) return updates;

        _checkTicksInitialized(uni, id, rewards);
        _checkSortedUnique(rewards);

        int24 currentTick = uni.getSlot0(id).tick();

        // Ensure current tick update doesn't get separated into its own update.
        if (rewards[0].tick == currentTick) {
            updates = new RewardsUpdate[](1);
            updates[0] = _createRewardUpdateAbove(uni, id, rewards, currentTick);
            return updates;
        } else if (rewards[rewards.length - 1].tick == currentTick) {
            updates = new RewardsUpdate[](1);
            updates[0] = _createRewardUpdateBelow(uni, id, rewards, currentTick);
            return updates;
        }

        uint256 index = rewards.findTickGte(currentTick);
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
            updates[0] = _createRewardUpdateAbove(uni, id, rewardsAbove, currentTick);
            return updates;
        } else if (rewardsAbove.length == 0) {
            updates = new RewardsUpdate[](1);
            updates[0] = _createRewardUpdateBelow(uni, id, rewardsBelow, currentTick);
            return updates;
        } else {
            updates = new RewardsUpdate[](2);
            updates[0] = _createRewardUpdateAbove(uni, id, rewardsAbove, currentTick);
            updates[1] = _createRewardUpdateBelow(uni, id, rewardsBelow, currentTick);
            return updates;
        }
    }

    function _checkTicksInitialized(IPoolManager uni, PoolId id, TickReward[] memory rewards) private view {
        for (uint256 i = 0; i < rewards.length; i++) {
            int24 tick = rewards[i].tick;
            (int16 wordPos, uint8 bitPos) = TickLib.position(TickLib.compress(tick));
            require(uni.getPoolBitmapInfo(id, wordPos).isInitialized(bitPos), "Tick not initialized");
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

    function _createRewardUpdateBelow(IPoolManager uni, PoolId id, TickReward[] memory rewards, int24 currentTick)
        private
        view
        returns (RewardsUpdate memory update)
    {
        require(rewards.length > 0, "No rewards");

        update.below = true;

        UintVec memory amounts = VecLib.uint_with_cap(rewards.length * 2);

        uint256 i = 0;
        int24 tick = rewards[0].tick;
        bool initialized = false;
        bool startTickSet = false;
        int128 cumNetLiquidity = 0;

        while (tick <= currentTick) {
            if (initialized && startTickSet) {
                (, int128 liquidityNet) = uni.getTickLiquidity(id, tick);
                cumNetLiquidity += liquidityNet;
            }
            (initialized, tick) = uni.getNextTickUp(id, tick);
            if (initialized) {
                if (i < rewards.length) {
                    TickReward memory reward = rewards[i];
                    if (reward.tick < tick) {
                        if (!startTickSet) {
                            update.startTick = tick;
                            startTickSet = true;
                        }
                        amounts.push(reward.amount);
                        i++;
                    } else if (startTickSet) {
                        amounts.push(0);
                    }
                } else if (startTickSet) {
                    amounts.push(0);
                }
            }
        }

        require(i == rewards.length, "Not all rewards used?");

        update.startLiquidity = MixedSignLib.sub(uni.getPoolLiquidity(id), cumNetLiquidity);
        update.quantities = new uint128[](amounts.length);
        for (i = 0; i < amounts.length; i++) {
            update.quantities[i] = uint128(amounts.get(i));
        }
    }

    function _createRewardUpdateAbove(IPoolManager uni, PoolId id, TickReward[] memory rewards, int24 currentTick)
        private
        view
        returns (RewardsUpdate memory update)
    {
        require(rewards.length > 0, "No rewards");

        update.below = false;

        UintVec memory amounts = VecLib.uint_with_cap(rewards.length * 2);

        uint256 i = 0;
        int24 tick = update.startTick = rewards[rewards.length - 1].tick;
        bool initialized = true;
        int128 cumNetLiquidity = 0;

        while (currentTick < tick) {
            if (initialized) {
                (, int128 liquidityNet) = uni.getTickLiquidity(id, tick);
                cumNetLiquidity += liquidityNet;
                if (i < rewards.length) {
                    TickReward memory reward = rewards[rewards.length - i - 1];

                    if (tick == reward.tick) {
                        amounts.push(reward.amount);
                        i++;
                    } else {
                        amounts.push(0);
                    }
                }
            }
            (initialized, tick) = uni.getNextTickDown(id, tick);
        }

        if (i < rewards.length) {
            require(
                i + 1 == rewards.length && rewards[0].tick == currentTick, "Donating above in _createRewardUpdateBelow"
            );
            amounts.push(rewards[0].amount);
        } else {
            amounts.push(0);
        }

        update.startLiquidity = MixedSignLib.add(uni.getPoolLiquidity(id), cumNetLiquidity);
        update.quantities = new uint128[](amounts.length);
        for (i = 0; i < amounts.length; i++) {
            update.quantities[i] = uint128(amounts.get(i));
        }
    }
}
