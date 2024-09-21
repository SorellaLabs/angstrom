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
import {TEST_LOGS} from "src/modules/DevFlags.sol";

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
        if (TEST_LOGS) {
            int24 currentTick = uni.getSlot0(id).tick();
            console.log(
                "toUpdates [id: %x  tick: %s ~= %s]",
                uint256(PoolId.unwrap(id)),
                currentTick.toStr(),
                currentTick.normalize().toStr()
            );
        }
        updates = _toUpdates(rewards, uni, id);
        if (TEST_LOGS) {
            console.log("  rewards:");
            debug_logRewards(rewards);
            for (uint256 i = 0; i < updates.length; i++) {
                console.log("  updates.%s:", i);
                RewardsUpdate memory update = updates[i];
                console.log("    %s", update.below ? "below" : "above");
                console.log("    startTick: %s", update.startTick.toStr());
                console.log("    startLiquidity: %s", update.startLiquidity.fmtD(12, 21));
                console.log("    quantities:%s", update.quantities.length == 0 ? " -" : "");
                for (uint256 j = 0; j < update.quantities.length; j++) {
                    uint128 amount = update.quantities[j];
                    console.log("      amount: %s", amount.fmtD(9));
                }
            }
        }
    }

    function debug_logRewards(TickReward[] memory rewards) internal pure {
        if (rewards.length == 0) {
            console.log("    -");
        } else {
            for (uint256 i = 0; i < rewards.length; i++) {
                TickReward memory reward = rewards[i];
                console.log("    { tick: %s, amount: %s }", reward.tick.toStr(), reward.amount.fmtD(9));
            }
        }
    }

    function _toUpdates(TickReward[] memory rewards, IPoolManager uni, PoolId id)
        internal
        view
        returns (RewardsUpdate[] memory updates)
    {
        if (rewards.length == 0) return updates;

        _checkTicksInitialized(uni, id, rewards);
        _checkSortedUnique(rewards);

        int24 currentTick = uni.getSlot0(id).tick().normalize();

        // Ensure current tick update doesn't get separated into its own update.
        if (rewards[0].tick == currentTick) {
            updates = new RewardsUpdate[](1);
            updates[0] = createRewardUpdateAbove(uni, id, rewards, currentTick);
            return updates;
        } else if (rewards[rewards.length - 1].tick == currentTick) {
            updates = new RewardsUpdate[](1);
            updates[0] = createRewardUpdateBelow(uni, id, rewards, currentTick);
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
            updates[0] = createRewardUpdateAbove(uni, id, rewardsAbove, currentTick);
            return updates;
        } else if (rewardsAbove.length == 0) {
            updates = new RewardsUpdate[](1);
            updates[0] = createRewardUpdateBelow(uni, id, rewardsBelow, currentTick);
            return updates;
        } else {
            updates = new RewardsUpdate[](2);
            updates[0] = createRewardUpdateAbove(uni, id, rewardsAbove, currentTick);
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

    function createRewardUpdateBelow(IPoolManager uni, PoolId id, TickReward[] memory rewards, int24 currentTick)
        internal
        view
        returns (RewardsUpdate memory update)
    {
        console.log("  below:");
        debug_logRewards(rewards);
        require(rewards.length > 0, "No rewards");

        update.below = true;

        UintVec memory amounts = VecLib.uint_with_cap(rewards.length * 2);

        uint256 i = 0;
        int128 cumulativeNetLiquidity = 0;
        bool initialized;
        int24 tick = rewards[0].tick;

        int128 netTickLiquidity;
        while (tick <= currentTick) {
            (initialized, tick) = uni.getNextTickGt(id, tick);

            if (tick > currentTick) break;

            if (initialized) {
                if (i < rewards.length) {
                    TickReward memory reward = rewards[i];
                    if (tick > reward.tick) {
                        amounts.push(reward.amount);
                        if (i == 0) {
                            update.startTick = tick;
                        }
                        (, netTickLiquidity) = uni.getTickLiquidity(id, tick);
                        cumulativeNetLiquidity += netTickLiquidity;
                        i++;
                        continue;
                    }
                }
                (, netTickLiquidity) = uni.getTickLiquidity(id, tick);
                cumulativeNetLiquidity += netTickLiquidity;
                amounts.push(0);
            }
        }

        if (i < rewards.length) {
            TickReward memory reward = rewards[i];
            amounts.push(reward.amount);
            if (i == 0) {
                update.startTick = currentTick;
            }
            i++;
        } else {
            amounts.push(0);
        }

        require(i == rewards.length, "Not all rewards used?");

        update.startLiquidity = MixedSignLib.sub(uni.getPoolLiquidity(id), cumulativeNetLiquidity);
        update.quantities = new uint128[](amounts.length);
        for (i = 0; i < amounts.length; i++) {
            update.quantities[i] = uint128(amounts.get(i));
        }
    }

    function createRewardUpdateAbove(IPoolManager uni, PoolId id, TickReward[] memory rewards, int24 currentTick)
        internal
        view
        returns (RewardsUpdate memory update)
    {
        console.log("  above:");
        debug_logRewards(rewards);
        require(rewards.length > 0, "No rewards");

        update.below = false;

        UintVec memory amounts = VecLib.uint_with_cap(rewards.length * 2);

        uint256 lastIndex = rewards.length - 1;
        int24 tick = update.startTick = rewards[lastIndex].tick;
        amounts.push(rewards[lastIndex].amount);
        uint256 i = 1;
        bool initialized;
        int128 cumulativeNetLiquidity = 0;

        console.log("currentTick: %s", currentTick.toStr());
        console.log("tic: %s", tick.toStr());

        if (currentTick < tick) {
            while (true) {
                int128 netTickLiquidity;
                (, netTickLiquidity) = uni.getTickLiquidity(id, tick);
                cumulativeNetLiquidity += netTickLiquidity;

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
