// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

/// @author philogy <https://github.com/philogy>
library Utils {
    function brutalize(address addr) internal view returns (address baddr) {
        assembly ("memory-safe") {
            mstore(0x00, gas())
            let dirt := keccak256(0, 32)
            baddr := xor(shl(160, dirt), addr)
        }
    }

    function brutalize(uint64 x) internal view returns (uint64 bx) {
        assembly ("memory-safe") {
            mstore(0x00, gas())
            let dirt := keccak256(0, 32)
            bx := xor(shl(64, dirt), x)
        }
    }
}
