// SPDX-License-Identifier: MIT
pragma solidity 0.8.26;

import {UniConsumer} from "./UniConsumer.sol";
import {IBeforeInitializeHook} from "../interfaces/IHooks.sol";

import {PoolConfigStore, PoolConfigStoreLib, StoreKey} from "../libraries/PoolConfigStore.sol";
import {PoolKey} from "v4-core/src/types/PoolKey.sol";
import {Currency} from "v4-core/src/types/Currency.sol";
import {SafeCastLib} from "solady/src/utils/SafeCastLib.sol";
import {POOL_FEE} from "src/Constants.sol";

/// @author philogy <https://github.com/philogy>
abstract contract TopLevelAuth is UniConsumer, IBeforeInitializeHook {
    error NotController();
    error OnlyOncePerBlock();
    error NotNode();
    error InvalidPoolKey();

    address internal immutable _CONTROLLER;

    mapping(address => bool) internal _isNode;

    uint64 internal _lastBlockUpdated;
    PoolConfigStore internal _configStore;

    constructor(address controller) {
        _CONTROLLER = controller;
    }

    function beforeInitialize(
        address,
        PoolKey calldata poolKey,
        uint160,
        bytes calldata storeIndexBytes
    ) external view returns (bytes4) {
        _onlyUniV4();

        // Uniswap ensures that `currency0 < currency1`.
        StoreKey key = PoolConfigStoreLib.keyFromAssetsUnchecked(
            Currency.unwrap(poolKey.currency0), Currency.unwrap(poolKey.currency1)
        );
        (int24 tickSpacing,) = _configStore.get(key, uint16(bytes2(storeIndexBytes)));
        if (poolKey.tickSpacing != tickSpacing || poolKey.fee != POOL_FEE) revert InvalidPoolKey();
        return this.beforeInitialize.selector;
    }

    /// @dev Allow controller to set parameters of a given pool.
    function configurePool(address asset0, address asset1, uint16 tickSpacing, uint24 feeInE6)
        external
    {
        _onlyController();
        _configStore = _configStore.setIntoNew(asset0, asset1, tickSpacing, feeInE6);
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
