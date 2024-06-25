// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

/// @author philogy <https://github.com/philogy>
library Branchless {
    function or(bool a, bool b) internal pure returns (bool c) {
        assembly ("memory-safe") {
            c := or(a, b)
        }
    }

    function and(bool a, bool b) internal pure returns (bool c) {
        assembly ("memory-safe") {
            c := and(a, b)
        }
    }

    function into(bool a) internal pure returns (uint256 x) {
        assembly ("memory-safe") {
            x := a
        }
    }

    function andNot(bool a, bool b) internal pure returns (bool c) {
        assembly ("memory-safe") {
            c := gt(a, b)
        }
    }

    function andNot(bool a, uint256 x) internal pure returns (bool c) {
        assembly ("memory-safe") {
            c := gt(a, x)
        }
    }
}
