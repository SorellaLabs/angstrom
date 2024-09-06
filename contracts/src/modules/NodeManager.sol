// SPDX-License-Identifier: BUSL-1.1
pragma solidity 0.8.26;

import {SafeCastLib} from "solady/src/utils/SafeCastLib.sol";

/// @author philogy <https://github.com/philogy>
abstract contract NodeManager {
    error NotController();
    error OnlyOncePerBlock();
    error NotNode();

    address internal immutable _CONTROLLER;

    mapping(address => bool) internal _isNode;

    uint64 public lastBlockUpdated;
    uint96 public halfSpreadRay;

    constructor(address controller) {
        _CONTROLLER = controller;
    }

    modifier onlyController() {
        if (msg.sender != _CONTROLLER) revert NotController();
        _;
    }

    function govUpdateHalfSpread(uint96 newHalfSpreadRay) external onlyController {
        halfSpreadRay = newHalfSpreadRay;
    }

    function govToggleNodes(address[] calldata nodes) external onlyController {
        for (uint256 i = 0; i < nodes.length; i++) {
            address node = nodes[i];
            _isNode[node] = !_isNode[node];
        }
    }

    /// @dev Validates that the caller is a node and that the last call is at least 1 block old.
    /// Blocks reentrant calls as well as separate calls in the same block.
    function _nodeBundleLock() internal {
        // Block-wide reentrancy lock. Prevents general reentrancy and replay of flash orders.
        if (lastBlockUpdated == block.number) revert OnlyOncePerBlock();
        if (!_isNode[msg.sender]) revert NotNode();
        lastBlockUpdated = SafeCastLib.toUint64(block.number);
    }
}
