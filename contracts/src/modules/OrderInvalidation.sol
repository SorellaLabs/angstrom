// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

/// @author philogy <https://github.com/philogy>
abstract contract OrderInvalidation {
    error NonceReuse();
    error OrderAlreadyExecuted();
    error Expired();
    error TWAPNonceReuse();
    error TWAPExpired();
    error InvalidTWAPNonce();
    error InvalidTWAPOrder();

    /// @dev `keccak256("angstrom-v1_0.unordered-nonces.slot")[0:4]`
    uint256 private constant UNORDERED_NONCES_SLOT = 0xdaa050e9;
    /// @dev `keccak256("angstrom-v1_0.twap-unordered-nonces.slot")[0:4]`
    uint256 private constant UNORDERED_TWAP_NONCES_SLOT = 0x635a0808;
    // type(uint32).max
    uint256 private constant MASK_U32 = 4294967295;
    // type(uint40).max
    uint256 private constant MASK_U40 = 1099511627775;
    // type(uint64).max
    uint256 private constant MASK_U64 = 18446744073709551615;
    // type(uint232).max
    uint256 private constant MASK_U232 = 6901746346790563787434755862277025452451108972170386555162524223799295;
    // max upper limit of twap intervals = once very 365.25 days
    uint256 private constant MAX_TWAP_INTERVAL = 0x1e187e0;
    // max no. of order parts = 365.25 days / 5 seconds
    uint256 private constant MAX_TWAP_TOTAL_PARTS = 0x604e60;

    function invalidateNonce(uint64 nonce) external {
        _invalidateNonce(msg.sender, nonce);
    }

    function invalidateTWAPNonce(uint64 nonce) external {
        assembly ("memory-safe") {
            nonce := and(nonce, MASK_U64)
            mstore(12, div(nonce, 232))
            mstore(4, UNORDERED_TWAP_NONCES_SLOT)
            mstore(0, caller())

            let bitmapPtr := keccak256(12, 32)
            let flag := shl(mod(nonce, 232), 1)
            let bitmapVal := sload(bitmapPtr)
            let updated := xor(and(bitmapVal, MASK_U232) , flag)
            let twapNonce := iszero(and(updated, flag))
            let fParts := shr(232, bitmapVal)

            if xor(iszero(iszero(fParts)), twapNonce) {
                mstore(0x00, 0xcfa42043 /* InvalidTWAPNonce() */ )
                revert(0x1c, 0x04)
            }

            if eq(fParts, 0xffffff) {
                mstore(0x00, 0x9a495418 /* TWAPNonceReuse() */ )
                revert(0x1c, 0x04)
            }

            sstore(bitmapPtr, or(updated, shl(232, 0xffffff)))
        }
    }

    function _checkTWAPOrderData(uint32 interval, uint32 tParts) internal pure {
        bool validInterval = interval != 0 && interval < MAX_TWAP_INTERVAL;
        bool validTParts = tParts != 0 && tParts < MAX_TWAP_TOTAL_PARTS;

        assembly {
            if iszero(and(validInterval, validTParts)){
                mstore(0x00, 0x51e490f3 /* InvalidTWAPOrder() */ )
                revert(0x1c, 0x04)
            }
        }
    }

    function _checkDeadline(uint256 deadline) internal view {
        if (block.timestamp > deadline) revert Expired();
    }

    function _invalidateNonce(address owner, uint64 nonce) internal {
        assembly ("memory-safe") {
            mstore(12, nonce)
            mstore(4, UNORDERED_NONCES_SLOT)
            mstore(0, owner)
            // Memory slice implicitly chops off last byte of `nonce` so that it's the nonce word
            // index (nonce >> 8).
            let bitmapPtr := keccak256(12, 31)
            let flag := shl(and(nonce, 0xff), 1)
            let updated := xor(sload(bitmapPtr), flag)

            if iszero(and(updated, flag)) {
                mstore(0x00, 0x8cb88872 /* NonceReuse() */ )
                revert(0x1c, 0x04)
            }

            sstore(bitmapPtr, updated)
        }
    }

    function _invalidatePartTWAPNonceAndCheckDeadline(address owner, uint64 nonce, uint40 sTime, uint32 interval, uint32 tParts) 
        internal 
    {
        uint256 _fParts;
        assembly ("memory-safe") {
            nonce := and(nonce, MASK_U64)
            mstore(12, div(nonce, 232))
            mstore(4, UNORDERED_TWAP_NONCES_SLOT)
            mstore(0, owner)

            let bitmapPtr := keccak256(12, 32)
            let flag := shl(mod(nonce, 232), 1)
            let bitmapVal := sload(bitmapPtr)
            let updated := xor(and(bitmapVal, MASK_U232), flag)
            let twapNonce := iszero(and(updated, flag))

            // part to fulfill
            let fParts := shr(232, bitmapVal)
            _fParts := fParts

            if xor(iszero(iszero(fParts)), twapNonce) {
                mstore(0x00, 0xcfa42043 /* InvalidTWAPNonce() */ )
                revert(0x1c, 0x04)
            }
            
            fParts := add(fParts, 1)
            tParts:= and(tParts, MASK_U32)

            if gt(fParts, tParts) {
                mstore(0x00, 0x9a495418 /* TWAPNonceReuse() */ )
                revert(0x1c, 0x04)
            }

            updated := or(shl(232, fParts), flag)

            if iszero(sub(tParts, fParts)) {
                updated := or(updated, shl(232, 0xffffff))
            }
            sstore(bitmapPtr, updated)
        }

        assembly ("memory-safe") {
            let cPartStart := add(and(sTime, MASK_U40), mul(_fParts, and(interval, MASK_U32)))
            
            if or(lt(timestamp(), cPartStart), gt(timestamp(), add(cPartStart, interval))) {
                mstore(0x00, 0x982c606d /* TWAPExpired() */ )
                revert(0x1c, 0x04)
            }
        }
    }

    function _invalidateOrderHash(bytes32 orderHash, address from) internal {
        assembly ("memory-safe") {
            mstore(20, from)
            mstore(0, orderHash)
            let slot := keccak256(0, 52)
            if tload(slot) {
                mstore(0x00, 0x8a2ef116 /* OrderAlreadyExecuted() */ )
                revert(0x1c, 0x04)
            }
            tstore(slot, 1)
        }
    }
}
