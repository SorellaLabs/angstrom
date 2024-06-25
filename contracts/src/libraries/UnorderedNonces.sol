// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {pptr} from "./Pointers.sol";

/// @author philogy <https://github.com/philogy>
abstract contract UnorderedNonces {
    error NonceReuse();

    /// @dev `keccak256("angstrom-v1_0.unordered-nonces.slot")[0:4]`
    uint256 private constant UNORDERED_NONCES_SLOT = 0xdaa050e9;

    function _useNonce(address owner, uint64 nonce) internal {
        pptr bitmap = _getBitmapPtr(owner, nonce);
        uint256 flag = 1 << uint256(nonce & 0xff);
        uint256 updated = bitmap.load() ^ flag;
        if (updated & flag == 0) revert NonceReuse();
        bitmap.store(updated);
    }

    function _getBitmapPtr(address owner, uint64 nonce) internal pure returns (pptr ptr) {
        assembly ("memory-safe") {
            mstore(12, nonce)
            mstore(4, UNORDERED_NONCES_SLOT)
            mstore(0, owner)
            // Memory slice implicitly chops off last byte of `nonce` so that it's the nonce word
            // index (nonce >> 8).
            ptr := keccak256(12, 31)
        }
    }
}
