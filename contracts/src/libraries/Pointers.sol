// SPDX-License-Identifier: MIT
pragma solidity ^0.8.4;

/// @dev Persistent storage (prev. just "storage") pointer.
type pptr is uint256;

/// @dev Memory pointer.
type mptr is uint256;

using Pointers for pptr global;
using Pointers for mptr global;

/// @author philogy <https://github.com/philogy>
library Pointers {
    function toPersistent(uint256 slot) internal pure returns (pptr) {
        return pptr.wrap(slot);
    }

    function toPersistent(bytes32 slot) internal pure returns (pptr) {
        return pptr.wrap(uint256(slot));
    }

    function load(pptr ptr) internal view returns (uint256 value) {
        assembly {
            value := sload(ptr)
        }
    }

    function store(pptr ptr, uint256 value) internal {
        assembly {
            sstore(ptr, value)
        }
    }

    function load(mptr ptr) internal pure returns (uint256 value) {
        // I assume pointers will always be used in a memory safe way.
        assembly ("memory-safe") {
            value := mload(ptr)
        }
    }

    function store(mptr ptr, uint256 value) internal pure {
        // I assume pointers will always be used in a memory safe way.
        assembly ("memory-safe") {
            mstore(ptr, value)
        }
    }
}
