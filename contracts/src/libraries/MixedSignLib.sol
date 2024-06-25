// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

library MixedSignLib {
    error ArithmeticOverflowUnderflow();

    function add(uint256 x, int256 y) internal pure returns (uint256 z) {
        assembly ("memory-safe") {
            z := add(x, y)

            if iszero(eq(gt(z, x), sgt(y, 0))) {
                mstore(0x00, 0xc9654ed4 /* ArithmeticOverflowUnderflow() */ )
                revert(0x1c, 0x04)
            }
        }
    }

    function sub(uint256 x, int256 y) internal pure returns (uint256 z) {
        assembly ("memory-safe") {
            z := sub(x, y)

            if iszero(eq(gt(z, x), slt(y, 0))) {
                mstore(0x00, 0xc9654ed4 /* ArithmeticOverflowUnderflow() */ )
                revert(0x1c, 0x04)
            }
        }
    }
}
