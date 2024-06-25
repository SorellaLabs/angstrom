// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

/// @author philogy <https://github.com/philogy>
library Utils {
    function brutalize(address addr) internal view returns (address baddr) {
        assembly ("memory-safe") {
            baddr := xor(addr, shl(160, gas()))
        }
    }

    function brutalize(uint64 x) internal view returns (uint64 bx) {
        assembly ("memory-safe") {
            bx := xor(x, shl(64, gas()))
        }
    }
}
