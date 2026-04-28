// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {PoolId, PoolIdLibrary} from "v4-core/src/types/PoolId.sol";
import {PoolKey} from "v4-core/src/types/PoolKey.sol";

import {IPoolManager} from "../interfaces/IUniV4.sol";
import {IAngstromAuth} from "../interfaces/IAngstromAuth.sol";
import {PoolConfigStore} from "../libraries/PoolConfigStore.sol";
import {StoreKey} from "../libraries/PoolConfigStore.sol";
import {X128MathLib} from "../libraries/X128MathLib.sol";
import {Positions, Position} from "../types/Positions.sol";
import {StateLibrary} from "v4-core/src/libraries/StateLibrary.sol";
import {AngstromView} from "./AngstromView.sol";

/// @notice Wrapper of `AngstromView` library, exposing the library functions as public functions.
/// @dev You can call this contract to query various aspects of Angstrom state. For any state not
/// covered by this library, you can use Angstrom's extsload function.
contract AngstromInspector {
    using AngstromView for IAngstromAuth;

    IAngstromAuth public immutable angstrom;
    IPoolManager public immutable uniswapPoolManager;

    constructor(IAngstromAuth _angstrom, IPoolManager _uniswapPoolManager) {
        angstrom = _angstrom;
        uniswapPoolManager = _uniswapPoolManager;
    }

    function controller() public view returns (address) {
        return angstrom.controller();
    }

    function lastBlockUpdated() public view returns (uint64) {
        return angstrom.lastBlockUpdated();
    }

    function configStore() public view returns (PoolConfigStore addr) {
        return angstrom.configStore();
    }

    function unlockedFee(StoreKey key) public view returns (uint24 fee, uint24 protocolFee) {
        return angstrom.unlockedFee(key);
    }

    function balanceOf(address asset, address owner) public view returns (uint256) {
        return angstrom.balanceOf(asset, owner);
    }

    /// @notice Returns poolRewards[id].globalGrowth
    function poolRewardsGlobalGrowth(PoolId id) public view returns (uint256) {
        return angstrom.poolRewardsGlobalGrowth(id);
    }

    /// @notice Returns poolRewards[id].rewardGrowthOutside[tick]
    function poolRewardsGrowthOutside(PoolId id, uint24 tick) public view returns (uint256) {
        return angstrom.poolRewardsGrowthOutside(id, tick);
    }

    /// @notice Returns the rewards growth inside of `lowerTick` and `upperTick`, given the `currentTick`,
    /// for the pool identified by `id`
    function getGrowthInside(PoolId id, int24 currentTick, int24 lowerTick, int24 upperTick)
        public
        view
        returns (uint256 growthInside)
    {
        return angstrom.getGrowthInside(id, currentTick, lowerTick, upperTick);
    }

    /// @notice Returns positions[id][uniPositionKey].lastGrowthInside
    function getLastGrowthInside(PoolId id, bytes32 uniPositionKey) public view returns (uint256) {
        return angstrom.getLastGrowthInside(id, uniPositionKey);
    }

    /// @notice Returns the total pending rewards of the position defined  by the `(account, key, tickLower, tickUpper, salt)` tuple.
    /// @dev Sums the return values of `pendingRewards` and `uniswapPendingRewards`.
    function totalPendingRewards(
        address account,
        PoolKey memory key,
        int24 tickLower,
        int24 tickUpper,
        bytes32 salt
    ) public view returns (uint256, uint256) {
        return angstrom.totalPendingRewards(account, key, tickLower, tickUpper, salt, uniswapPoolManager);
    }

    /// @notice Returns the pending rewards held by the Angstrom hook for the position defined
    /// by the `(account, key, tickLower, tickUpper, salt)` tuple.
    /// @dev Rewards from Angstrom are always in the Currency0 of the pair.
    function pendingRewards(
        address account,
        PoolKey memory key,
        int24 tickLower,
        int24 tickUpper,
        bytes32 salt
    ) public view returns (uint256) {
        return angstrom.pendingRewards(account, key, tickLower, tickUpper, salt, uniswapPoolManager);
    }

    function uniswapPendingRewards(
        address account,
        PoolKey memory key,
        int24 tickLower,
        int24 tickUpper,
        bytes32 salt
    ) public view returns (uint256, uint256) {
        return AngstromView.uniswapPendingRewards(account, key, tickLower, tickUpper, salt, uniswapPoolManager);
    }
}
