// SPDX-License-Identifier: BUSL-1.1
pragma solidity 0.8.24;

import {MerkleProofLib} from "solady/src/utils/MerkleProofLib.sol";
import {tuint256} from "transient-goodies/TransientPrimitives.sol";

/// @author philogy <https://github.com/philogy>
abstract contract NodeManager {
    error NotGovernance();
    error NotNode();

    mapping(address => bool) internal _isNode;

    address internal immutable _GOVERNANCE;

    constructor(address governance) {
        _GOVERNANCE = governance;
    }

    modifier onlyNode() {
        if (!_isNode[msg.sender]) revert NotNode();
        _;
    }

    modifier onlyGovernance() {
        if (msg.sender != _GOVERNANCE) revert NotGovernance();
        _;
    }

    function updateNodes(address[] calldata nodes) external onlyGovernance {
        for (uint256 i = 0; i < nodes.length; i++) {
            address node = nodes[i];
            _isNode[node] = !_isNode[node];
        }
    }
}
