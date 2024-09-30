// SPDX-License-Identifier: BUSL-1.1
pragma solidity 0.8.26;

import {
    PoolConfigStore,
    PoolConfigStoreLib,
    StoreKey
} from "../libraries/pool-config/PoolConfigStore.sol";
import {SafeCastLib} from "solady/src/utils/SafeCastLib.sol";

/// @author philogy <https://github.com/philogy>
abstract contract NodeManager {
    error NotController();
    error OnlyOncePerBlock();
    error NotNode();

    address internal immutable _CONTROLLER;

    mapping(address => bool) internal _isNode;

    uint64 internal _lastBlockUpdated;
    PoolConfigStore internal _configStore;

    constructor(address controller) {
        _CONTROLLER = controller;
    }

    modifier onlyController() {
        if (msg.sender != _CONTROLLER) revert NotController();
        _;
    }

    /// @dev Allow controller to set parameters of a given pool.
    function configurePool(address asset0, address asset1, uint16 tickSpacing, uint24 feeInE6)
        external
        onlyController
    {
        _configStore = _configStore.setIntoNew(asset0, asset1, tickSpacing, feeInE6);
    }

    function toggleNodes(address[] calldata nodes) external onlyController {
        for (uint256 i = 0; i < nodes.length; i++) {
            address node = nodes[i];
            _isNode[node] = !_isNode[node];
        }
    }

    /// @dev Validates that the caller is a node and that the last call is at least 1 block old.
    /// Blocks reentrant calls as well as separate calls in the same block.
    function _nodeBundleLock() internal {
        // Block-wide reentrancy lock. Prevents general reentrancy and replay of flash orders.
        if (_lastBlockUpdated == block.number) revert OnlyOncePerBlock();
        if (!_isNode[msg.sender]) revert NotNode();
        _lastBlockUpdated = SafeCastLib.toUint64(block.number);
    }
}
