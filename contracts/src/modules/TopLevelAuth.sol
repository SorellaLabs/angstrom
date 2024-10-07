// SPDX-License-Identifier: MIT
pragma solidity 0.8.26;

import {UniConsumer} from "./UniConsumer.sol";
import {IBeforeInitializeHook} from "../interfaces/IHooks.sol";

import {PoolConfigStore, PoolConfigStoreLib, StoreKey} from "../libraries/PoolConfigStore.sol";
import {IHooks} from "v4-core/src/interfaces/IHooks.sol";
import {PoolKey} from "v4-core/src/types/PoolKey.sol";
import {SafeCastLib} from "solady/src/utils/SafeCastLib.sol";
import {POOL_FEE} from "src/Constants.sol";

/// @author philogy <https://github.com/philogy>
abstract contract TopLevelAuth is UniConsumer, IBeforeInitializeHook {
    error NotController();
    error OnlyOncePerBlock();
    error NotNode();
    error InvalidPoolKey();
    error NotFromHook();
    error IndexMayHaveChanged();

    address internal immutable _CONTROLLER;

    mapping(address => bool) internal _isNode;

    uint64 internal _lastBlockUpdated;
    PoolConfigStore internal _configStore;

    constructor(address controller) {
        _CONTROLLER = controller;
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
            PoolKey(_c(assetA), _c(assetB), POOL_FEE, tickSpacing, IHooks(address(this))),
            sqrtPriceX96,
            ""
        );
    }

    function beforeInitialize(address caller, PoolKey calldata, uint160, bytes calldata)
        external
        view
        returns (bytes4)
    {
        _onlyUniV4();

        if (caller != address(this)) revert NotFromHook();

        return this.beforeInitialize.selector;
    }

    function removePool(address expectedStore, uint256 storeIndex) external {
        _onlyController();
        PoolConfigStore store = _configStore;
        if (PoolConfigStore.unwrap(store) != expectedStore) revert IndexMayHaveChanged();
        _configStore = store.removeIntoNew(storeIndex);
    }

    /// @dev Allow controller to set parameters of a given pool.
    function configurePool(address assetA, address assetB, uint16 tickSpacing, uint24 feeInE6)
        external
    {
        _onlyController();
        _configStore = _configStore.setIntoNew(assetA, assetB, tickSpacing, feeInE6);
    }

    function toggleNodes(address[] calldata nodes) external {
        _onlyController();
        for (uint256 i = 0; i < nodes.length; i++) {
            address node = nodes[i];
            _isNode[node] = !_isNode[node];
        }
    }

    function _onlyController() internal view {
        if (msg.sender != _CONTROLLER) revert NotController();
    }

    /// @dev Validates that the caller is a node and that the last call is at least 1 block old.
    /// Blocks reentrant calls as well as separate calls in the same block.
    function _nodeBundleLock() internal {
        if (_lastBlockUpdated == block.number) revert OnlyOncePerBlock();
        if (!_isNode[msg.sender]) revert NotNode();
        _lastBlockUpdated = SafeCastLib.toUint64(block.number);
    }
}
