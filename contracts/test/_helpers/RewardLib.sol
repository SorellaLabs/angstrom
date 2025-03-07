// SPDX-License-Identifier: BUSL-1.1
pragma solidity ^0.8.0;

import {PoolId} from "v4-core/src/types/PoolId.sol";
import {IPoolManager} from "v4-core/src/interfaces/IPoolManager.sol";
import {IUniV4} from "../../src/interfaces/IUniV4.sol";
import {RewardsUpdate} from "test/_reference/PoolUpdate.sol";
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

    uint256 internal constant MAX_LOOP = 120;

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

    function CurrentOnly(uint128 amount) internal pure returns (RewardsUpdate memory update) {
        update.onlyCurrent = true;
        update.onlyCurrentQuantity = amount;
    }

    function toUpdates(TickReward[] memory rewards, IPoolManager uni, PoolId id, int24 tickSpacing)
        internal
        view
        returns (RewardsUpdate[] memory updates)
    {
        updates = toUpdates(rewards, uni, id, tickSpacing, uni.getSlot0(id).tick());
    }

    function toUpdates(
        TickReward[] memory rewards,
        IPoolManager uni,
        PoolId id,
        int24 tickSpacing,
        int24 currentTick
    ) internal view returns (RewardsUpdate[] memory updates) {
        require(tickSpacing >= 1, "Invalid TICK_SPACING");
        if (rewards.length == 0) return updates;

        _checkTicksInitialized(uni, id, rewards, tickSpacing);
        _checkSortedUnique(rewards);

        // Ensure current tick update doesn't get separated into its own update.
        if (rewards[rewards.length - 1].tick <= currentTick) {
            updates = new RewardsUpdate[](1);
            updates[0] = _createRewardUpdateBelow(uni, id, rewards, currentTick, tickSpacing);
            return updates;
        } else if (currentTick <= rewards[0].tick) {
            updates = new RewardsUpdate[](1);
            updates[0] = _createRewardUpdateAbove(uni, id, rewards, currentTick, tickSpacing);
            return updates;
        } else {
            // From the following facts:
            // - all ticks in rewards list are initialized
            // - `currentTick` is in between ticks in the list
            // Deduce that `currentTick` is *inside* an initialized range.
            // Therefore the following loop searching for the start of the current liquidity range
            // *will* terminate.
            int24 currentRangeStartTick = currentTick;
            bool initialized;
            while (true) {
                (initialized, currentRangeStartTick) =
                    uni.getNextTickLe(id, currentRangeStartTick, tickSpacing);
                if (initialized) break;
                currentRangeStartTick--;
            }

            // Could potentially be able to able do a single above donate now.
            if (currentRangeStartTick <= rewards[0].tick) {
                updates = new RewardsUpdate[](1);
                updates[0] = _createRewardUpdateAbove(uni, id, rewards, currentTick, tickSpacing);
                return updates;
            }
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
            updates[0] = _createRewardUpdateAbove(uni, id, rewardsAbove, currentTick, tickSpacing);
            return updates;
        } else if (rewardsAbove.length == 0) {
            updates = new RewardsUpdate[](1);
            updates[0] = _createRewardUpdateBelow(uni, id, rewardsBelow, currentTick, tickSpacing);
            return updates;
        } else {
            updates = new RewardsUpdate[](2);
            updates[0] = _createRewardUpdateAbove(uni, id, rewardsAbove, currentTick, tickSpacing);
            updates[1] = _createRewardUpdateBelow(uni, id, rewardsBelow, currentTick, tickSpacing);
            return updates;
        }
    }

    function _checkTicksInitialized(
        IPoolManager uni,
        PoolId id,
        TickReward[] memory rewards,
        int24 tickSpacing
    ) private view {
        for (uint256 i = 0; i < rewards.length; i++) {
            int24 tick = rewards[i].tick;
            (int16 wordPos, uint8 bitPos) = TickLib.position(TickLib.compress(tick, tickSpacing));
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

    function _createRewardUpdateBelow(
        IPoolManager uni,
        PoolId id,
        TickReward[] memory rewards,
        int24 currentTick,
        int24 tickSpacing
    ) private view returns (RewardsUpdate memory update) {
        require(rewards.length > 0, "No rewards");

        // Create list of initialized ticks, including start (checked before) and the tick of the
        // current range.
        UintVec memory initializedTicks = VecLib.uint_with_cap(rewards.length * 3 / 2);
        {
            int24 tick = rewards[0].tick;
            bool initialized = true;
            uint256 uninit = 0;
            while (true) {
                (initialized, tick) = uni.getNextTickGt(id, tick, tickSpacing);
                if (currentTick < tick) break;
                if (initialized) {
                    initializedTicks.push(uint256(int256(tick)));
                    uninit = 0;
                } else {
                    uninit++;
                    require(uninit <= MAX_LOOP, "MAX_LOOP exceeded in _createRewardUpdateBelow");
                }
            }
        }

        if (initializedTicks.length == 0) {
            require(rewards.length == 1, "expected rewards length 1");
            return CurrentOnly(rewards[0].amount);
        }

        update.quantities = new uint128[](initializedTicks.length + 1);

        uint256 ri = 0;
        int128 cumulativeNetLiquidity = 0;

        for (uint256 i = 0; i < initializedTicks.length; i++) {
            int24 tick = int24(int256(initializedTicks.get(i)));
            int128 tickNetLiquidity;
            (, tickNetLiquidity) = uni.getTickLiquidity(id, tick);
            cumulativeNetLiquidity += tickNetLiquidity;
            if (ri < rewards.length) {
                TickReward memory reward = rewards[ri];
                if (reward.tick < tick) {
                    update.quantities[i] = reward.amount;
                    ri++;
                }
            } else {
                // Amounts in quantities array default to 0, leave them.
            }
        }

        if (ri < rewards.length) {
            update.quantities[initializedTicks.length] = rewards[ri].amount;
            ri++;
        }

        require(ri == rewards.length, "Not all rewards used?");

        update.startTick = int24(uint24(initializedTicks.get(0)));
        uint128 poolLiq = getLiquidityAtTick(uni, id, currentTick, tickSpacing);
        update.startLiquidity = MixedSignLib.sub(poolLiq, cumulativeNetLiquidity);

        {
            PoolId id2 = id;
            IPoolManager uni2 = uni;
            bytes32 rewardChecksum;
            uint128 liquidity = update.startLiquidity;
            for (uint256 i = 0; i < initializedTicks.length; i++) {
                int24 tick = int24(int256(initializedTicks.get(i)));
                (, int128 netLiquidity) = uni2.getTickLiquidity(id2, tick);
                liquidity = MixedSignLib.add(liquidity, netLiquidity);
                rewardChecksum = keccak256(abi.encodePacked(rewardChecksum, liquidity, tick));
            }
            update.rewardChecksum = uint80(uint256(rewardChecksum) >> 176);
        }
    }

    function _createRewardUpdateAbove(
        IPoolManager uni,
        PoolId id,
        TickReward[] memory rewards,
        int24 currentTick,
        int24 tickSpacing
    ) private view returns (RewardsUpdate memory update) {
        require(rewards.length > 0, "No rewards");

        // Create list of initialized ticks, including start (checked before) and the tick of the
        // current range.
        UintVec memory initializedTicks = VecLib.uint_with_cap(rewards.length * 3 / 2);
        int24 startTick = rewards[rewards.length - 1].tick;
        {
            bool initialized = true;
            int24 tick = startTick;
            uint256 uninit = 0;
            while (currentTick < tick) {
                if (initialized) {
                    initializedTicks.push(uint256(int256(tick)));
                    uninit = 0;
                } else {
                    uninit++;
                    require(uninit <= MAX_LOOP, "MAX_LOOP exceeded in _createRewardUpdateAbove");
                }
                (initialized, tick) = uni.getNextTickLt(id, tick, tickSpacing);
            }
        }

        if (initializedTicks.length == 0) {
            console.log("WARNING\nWARNING: Above somehow called with donate to current only???");
            require(rewards.length == 1, "Expected exact one reward");
            return CurrentOnly(rewards[0].amount);
        }

        update.startTick = startTick;
        update.quantities = new uint128[](initializedTicks.length + 1);

        uint256 ri = rewards.length;
        int128 cumulativeNetLiquidity = 0;

        for (uint256 i = 0; i < initializedTicks.length; i++) {
            int24 tick = int24(int256(initializedTicks.get(i)));
            int128 tickNetLiquidity;
            (, tickNetLiquidity) = uni.getTickLiquidity(id, tick);
            cumulativeNetLiquidity += tickNetLiquidity;
            if (ri > 0) {
                TickReward memory reward = rewards[ri - 1];
                if (reward.tick >= tick) {
                    update.quantities[i] = reward.amount;
                    ri--;
                }
            } else {
                // Amounts in quantities array default to 0, leave them.
            }
        }

        if (ri > 0) {
            update.quantities[initializedTicks.length] = rewards[0].amount;
            ri--;
        }

        require(ri == 0, "Not all rewards used?");

        uint128 poolLiq = getLiquidityAtTick(uni, id, currentTick, tickSpacing);
        update.startLiquidity = MixedSignLib.add(poolLiq, cumulativeNetLiquidity);

        {
            PoolId id2 = id;
            IPoolManager uni2 = uni;
            bytes32 rewardChecksum;
            uint128 liquidity = update.startLiquidity;
            for (uint256 i = 0; i < initializedTicks.length; i++) {
                int24 tick = int24(int256(initializedTicks.get(i)));
                (, int128 netLiquidity) = uni2.getTickLiquidity(id2, tick);
                liquidity = MixedSignLib.sub(liquidity, netLiquidity);
                rewardChecksum = keccak256(abi.encodePacked(rewardChecksum, liquidity, tick));
            }
            update.rewardChecksum = uint80(uint256(rewardChecksum) >> 176);
        }
    }

    function getLiquidityAtTick(IPoolManager uni, PoolId id, int24 futureTick, int24 tickSpacing)
        internal
        view
        returns (uint128)
    {
        int24 presentTick = uni.getSlot0(id).tick();
        uint128 realCurrentLiq = uni.getPoolLiquidity(id);
        if (presentTick < futureTick) {
            bool initialized;
            int24 tick = presentTick;
            uint256 uninit = 0;
            while (true) {
                (initialized, tick) = uni.getNextTickGt(id, tick, tickSpacing);
                if (futureTick < tick) break;
                if (initialized) {
                    uninit = 0;
                    (, int128 tickNetLiquidity) = uni.getTickLiquidity(id, tick);
                    realCurrentLiq = MixedSignLib.add(realCurrentLiq, tickNetLiquidity);
                } else {
                    uninit++;
                    require(
                        uninit <= MAX_LOOP,
                        "MAX_LOOP exceeded in getLiquidityAtTick [present < future]"
                    );
                }
            }
        } else if (futureTick < presentTick) {
            bool initialized = true;
            int24 tick = presentTick;
            uint256 uninit = 0;
            while (true) {
                (initialized, tick) = uni.getNextTickLe(id, tick, tickSpacing);
                if (tick <= futureTick) break;
                if (initialized) {
                    uninit = 0;
                    (, int128 tickNetLiquidity) = uni.getTickLiquidity(id, tick);
                    realCurrentLiq = MixedSignLib.sub(realCurrentLiq, tickNetLiquidity);
                } else {
                    uninit++;
                    require(
                        uninit <= MAX_LOOP,
                        "MAX_LOOP exceeded in getLiquidityAtTick [future < present]"
                    );
                }
                tick--;
            }
        }

        return realCurrentLiq;
    }

    function toStr(TickReward memory reward) internal pure returns (string memory) {
        return string.concat(
            "TickReward { tick: ", reward.tick.toStr(), ", amount: ", reward.amount.toStr(), " }"
        );
    }

    function re(TickReward memory reward) internal pure returns (TickReward[] memory r) {
        r = new TickReward[](1);
        r[0] = reward;
    }

    function re(TickReward memory r1, TickReward memory r2)
        internal
        pure
        returns (TickReward[] memory r)
    {
        r = new TickReward[](2);
        r[0] = r1;
        r[1] = r2;
    }

    function re(TickReward memory r1, TickReward memory r2, TickReward memory r3)
        internal
        pure
        returns (TickReward[] memory r)
    {
        r = new TickReward[](3);
        r[0] = r1;
        r[1] = r2;
        r[2] = r3;
    }
}
