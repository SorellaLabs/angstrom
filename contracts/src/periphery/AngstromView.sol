// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {PoolId, PoolIdLibrary} from "v4-core/src/types/PoolId.sol";
import {PoolKey} from "v4-core/src/types/PoolKey.sol";

import {IPoolManager, IUniV4} from "../interfaces/IUniV4.sol";
import {IAngstromAuth} from "../interfaces/IAngstromAuth.sol";
import {PoolConfigStore} from "../libraries/PoolConfigStore.sol";
import {StoreKey} from "../libraries/PoolConfigStore.sol";
import {X128MathLib} from "../libraries/X128MathLib.sol";
import {Positions, Position} from "../types/Positions.sol";

using IUniV4 for IPoolManager;
using PoolIdLibrary for PoolId;

/// @author philogy <https://github.com/philogy>
library AngstromView {
    uint256 constant CONTROLLER_SLOT = 0;

    uint256 constant IS_NODE_SLOT = 1;

    uint256 constant UNLOCKED_FEE_PACKED_SET_SLOT = 2;

    uint256 constant LAST_BLOCK_CONFIG_STORE_SLOT = 3;
    uint256 constant LAST_BLOCK_BIT_OFFSET = 0;
    uint256 constant STORE_BIT_OFFSET = 64;

    uint256 constant BALANCES_SLOT = 5;

    uint256 constant POSITIONS_SLOT = 6;

    uint256 constant POOL_REWARDS_SLOT = 7;

    uint256 constant REWARD_GROWTH_SIZE = 16777216;

    function controller(IAngstromAuth self) internal view returns (address) {
        return address(uint160(self.extsload(CONTROLLER_SLOT)));
    }

    function lastBlockUpdated(IAngstromAuth self) internal view returns (uint64) {
        return uint64(self.extsload(LAST_BLOCK_CONFIG_STORE_SLOT) >> LAST_BLOCK_BIT_OFFSET);
    }

    function configStore(IAngstromAuth self) internal view returns (PoolConfigStore addr) {
        uint256 value = self.extsload(LAST_BLOCK_CONFIG_STORE_SLOT);
        return PoolConfigStore.wrap(address(uint160(value >> STORE_BIT_OFFSET)));
    }

    function unlockedFee(IAngstromAuth self, StoreKey key)
        internal
        view
        returns (uint24 fee, uint24 protocolFee)
    {
        uint256 slot = uint256(keccak256(abi.encode(key, UNLOCKED_FEE_PACKED_SET_SLOT)));
        uint256 unlockedPackedIsSet = self.extsload(slot);
        fee = uint24(unlockedPackedIsSet);
        protocolFee = uint24(unlockedPackedIsSet >> 24);
    }

    function balanceOf(IAngstromAuth self, address asset, address owner)
        internal
        view
        returns (uint256)
    {
        bytes32 assetMapSlot = keccak256(abi.encode(asset, BALANCES_SLOT));
        bytes32 ownerBalanceSlot = keccak256(abi.encode(owner, assetMapSlot));
        return self.extsload(uint256(ownerBalanceSlot));
    }

    /// @notice Returns poolRewards[id].globalGrowth
    function poolRewardsGlobalGrowth(IAngstromAuth self, PoolId id) internal view returns (uint256) {
        uint256 rewardsMapSlot =
            uint256(keccak256(abi.encode(id, POOL_REWARDS_SLOT))) + REWARD_GROWTH_SIZE;
        return self.extsload(rewardsMapSlot);
    }

    /// @notice Returns poolRewards[id].rewardGrowthOutside[tick]
    function poolRewardsGrowthOutside(IAngstromAuth self, PoolId id, uint24 tick)
        internal
        view
        returns (uint256)
    {
        uint256 rewardsMapSlot =
            uint256(keccak256(abi.encode(id, POOL_REWARDS_SLOT))) + uint256(tick);
        return self.extsload(rewardsMapSlot);
    }

    /// @notice Returns the rewards growth inside of `lowerTick` and `upperTick`, given the `currentTick`,
    /// for the pool identified by `id`
    function getGrowthInside(
        IAngstromAuth self,
        PoolId id,
        int24 currentTick,
        int24 lowerTick,
        int24 upperTick
    ) internal view returns (uint256 growthInside) {
        unchecked {
            uint256 lowerTickGrowth = poolRewardsGrowthOutside(self, id, uint24(lowerTick));
            uint256 upperTickGrowth = poolRewardsGrowthOutside(self, id, uint24(upperTick));

            if (currentTick < lowerTick) {
                return lowerTickGrowth - upperTickGrowth;
            }
            if (upperTick <= currentTick) {
                return upperTickGrowth - lowerTickGrowth;
            }

            return poolRewardsGlobalGrowth(self, id) - lowerTickGrowth - upperTickGrowth;
        }
    }

    /// @notice Returns positions[id][uniPositionKey].lastGrowthInside
    function getLastGrowthInside(IAngstromAuth self, PoolId id, bytes32 uniPositionKey)
        internal
        view
        returns (uint256)
    {
        bytes32 positionKeyMapSlot = keccak256(abi.encode(id, POSITIONS_SLOT));
        bytes32 positionLastGrowthInsideSlot =
            keccak256(abi.encode(uniPositionKey, positionKeyMapSlot));
        return self.extsload(uint256(positionLastGrowthInsideSlot));
    }

    /// @notice Returns the pending rewards held by the Angstrom hook for the position defined
    /// by the `(account, key, tickLower, tickUpper, salt)` tuple.
    function pendingRewards(
        IAngstromAuth self,
        address account,
        PoolKey memory key,
        int24 tickLower,
        int24 tickUpper,
        bytes32 salt,
        IPoolManager uniV4
    ) internal view returns (uint256) {
        unchecked {
            PoolId id = key.toId();
            bytes32 uniPositionKey =
                keccak256(abi.encodePacked(account, tickLower, tickUpper, salt));
            int24 currentTick = uniV4.getSlot0(id).tick();
            uint256 growthInside = getGrowthInside(self, id, currentTick, tickLower, tickUpper);
            uint256 lastGrowthInside = getLastGrowthInside(self, id, uniPositionKey);
            uint128 positionTotalLiquidity = uniV4.getPositionLiquidity(id, uniPositionKey);
            uint256 rewards =
                X128MathLib.fullMulX128(growthInside - lastGrowthInside, positionTotalLiquidity);

            return rewards;
        }
    }
}
