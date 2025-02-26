// SPDX-License-Identifier: MIT
pragma solidity ^0.8.26;

import {IAngstromAuth} from "../interfaces/IAngstromAuth.sol";
import {UniConsumer} from "./UniConsumer.sol";

import {PoolConfigStore, PoolConfigStoreLib, StoreKey} from "../libraries/PoolConfigStore.sol";
import {IHooks} from "v4-core/src/interfaces/IHooks.sol";
import {PoolKey} from "v4-core/src/types/PoolKey.sol";
import {SafeCastLib} from "solady/src/utils/SafeCastLib.sol";
import {LPFeeLibrary} from "v4-core/src/libraries/LPFeeLibrary.sol";
import {SafeTransferLib} from "solady/src/utils/SafeTransferLib.sol";

/// @author philogy <https://github.com/philogy>
abstract contract TopLevelAuth is UniConsumer, IAngstromAuth {
    using LPFeeLibrary for uint24;
    using SafeTransferLib for address;

    error NotController();
    error UnlockedFeeNotSet(StoreKey key);
    error OnlyOncePerBlock();
    error NotNode();
    error IndexMayHaveChanged();

    /// @dev Contract that manages all special privileges for contract (setting new nodes,
    /// configuring pools, pulling fees).
    address internal _controller;

    mapping(address => bool) internal _isNode;
    mapping(StoreKey => uint256) private _unlockedFeePackedSet;

    uint64 internal _lastBlockUpdated;
    PoolConfigStore internal _configStore;

    constructor(address controller) {
        _controller = controller;
    }

    function setController(address newController) public {
        _onlyController();
        _controller = newController;
    }

    /// @dev Allow controller to set parameters of a given pool.
    function configurePool(
        address assetA,
        address assetB,
        uint16 tickSpacing,
        uint24 bundleFee,
        uint24 unlockedFee
    ) external {
        _onlyController();
        if (assetA > assetB) (assetA, assetB) = (assetB, assetA);
        StoreKey key = PoolConfigStoreLib.keyFromAssetsUnchecked(assetA, assetB);
        _configStore = _configStore.setIntoNew(key, assetA, assetB, tickSpacing, bundleFee);
        unlockedFee.validate();

        _unlockedFeePackedSet[key] = (uint256(unlockedFee) << 1) | 1;
    }

    function initializePool(
        address assetA,
        address assetB,
        uint256 storeIndex,
        uint160 sqrtPriceX96
    ) public {
        if (assetA > assetB) (assetA, assetB) = (assetB, assetA);
        StoreKey key = PoolConfigStoreLib.keyFromAssetsUnchecked(assetA, assetB);
        (int24 tickSpacing,) = _configStore.get(key, storeIndex);
        UNI_V4.initialize(
            PoolKey(_c(assetA), _c(assetB), INIT_HOOK_FEE, tickSpacing, IHooks(address(this))),
            sqrtPriceX96
        );
    }

    function removePool(StoreKey key, PoolConfigStore expectedStore, uint256 storeIndex) external {
        _onlyController();
        PoolConfigStore store = _configStore;
        // Validate entry.
        store.get(key, storeIndex);
        if (store != expectedStore) revert IndexMayHaveChanged();
        _configStore = store.removeIntoNew(storeIndex);
        _unlockedFeePackedSet[key] = 0;
    }

    /// @dev Function to allow controller to pull an arbitrary amount of tokens from the contract.
    /// Assumed to be accrued validator fees.
    function pullFee(address asset, uint256 amount) external {
        _onlyController();
        asset.safeTransfer(msg.sender, amount);
    }

    function toggleNodes(address[] calldata nodes) external {
        _onlyController();
        for (uint256 i = 0; i < nodes.length; i++) {
            address node = nodes[i];
            _isNode[node] = !_isNode[node];
        }
    }

    function _isUnlocked() internal view returns (bool) {
        return _lastBlockUpdated == block.number;
    }

    function _unlockedFee(address asset0, address asset1) internal view returns (uint24) {
        StoreKey key = PoolConfigStoreLib.keyFromAssetsUnchecked(asset0, asset1);
        uint256 packed = _unlockedFeePackedSet[key];
        if (packed & 1 == 0) revert UnlockedFeeNotSet(key);
        return uint24(packed >> 1);
    }

    function _onlyController() internal view {
        if (msg.sender != _controller) revert NotController();
    }

    /// @dev Validates that the caller is a node and that the last call is at least 1 block old.
    /// Blocks reentrant calls as well as separate calls in the same block.
    function _nodeBundleLock() internal {
        if (_lastBlockUpdated == block.number) revert OnlyOncePerBlock();
        if (!_isNode[msg.sender]) revert NotNode();
        _lastBlockUpdated = SafeCastLib.toUint64(block.number);
    }
}
