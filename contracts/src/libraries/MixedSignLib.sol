// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

library MixedSignLib {
    error ArithmeticOverflowUnderflow();

    function add(uint128 x, int128 y) internal pure returns (uint128 z) {
        assembly ("memory-safe") {
            z := add(x, y)

            if shr(128, z) {
                mstore(0x00, 0xc9654ed4 /* ArithmeticOverflowUnderflow() */ )
                revert(0x1c, 0x04)
            }
        }
    }

    function sub(uint128 x, int128 y) internal pure returns (uint128 z) {
        assembly ("memory-safe") {
            z := sub(x, y)

            if shr(128, z) {
                mstore(0x00, 0xc9654ed4 /* ArithmeticOverflowUnderflow() */ )
                revert(0x1c, 0x04)
            }
        }
    }
}
