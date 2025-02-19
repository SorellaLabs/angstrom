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
    uint256 private constant MASK_U32 = 0xffffffff;
    // type(uint40).max
    uint256 private constant MASK_U40 = 0xffffffffff;
    // type(uint64).max
    uint256 private constant MASK_U64 = 0xffffffffffffffff;
    // type(uint232).max
    uint256 private constant MASK_U232 = 0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffff;
    // max twap nonce bit = 232
    uint256 private constant MAX_TWAP_NONCE_SIZE = 0xe8;
    // max upper limit of twap intervals = 31557600 (365.25 days)
    uint256 private constant MAX_TWAP_INTERVAL = 0x1e187e0;
    // min lower limit of twap intervals = 12 seconds
    uint256 private constant MIN_TWAP_INTERVAL = 0x0c;
    // max no. of order parts = 6311520 (365.25 days / 5 seconds)
    uint256 private constant MAX_TWAP_TOTAL_PARTS = 0x604e60;

    function invalidateNonce(uint64 nonce) external {
        _invalidateNonce(msg.sender, nonce);
    }

    function invalidateTWAPNonce(uint64 nonce) external {
        assembly ("memory-safe") {
            nonce := and(nonce, MASK_U64)
            mstore(12, div(nonce, MAX_TWAP_NONCE_SIZE))
            mstore(4, UNORDERED_TWAP_NONCES_SLOT)
            mstore(0, caller())

            let bitmapPtr := keccak256(12, 32)
            let flag := shl(mod(nonce, MAX_TWAP_NONCE_SIZE), 1)
            let bitmapVal := sload(bitmapPtr)
            let updated := xor(and(bitmapVal, MASK_U232) , flag)
            let twapNonce := iszero(and(updated, flag))
            let fulfilledParts := shr(MAX_TWAP_NONCE_SIZE, bitmapVal)

            // Reverts if `fulfilledParts` is empty while `twapNonce` is not empty,
            // or if `fulfilledParts` is not empty while `twapNonce` is empty.
            if xor(iszero(iszero(fulfilledParts)), twapNonce) {
                mstore(0x00, 0xcfa42043 /* InvalidTWAPNonce() */ )
                revert(0x1c, 0x04)
            }

            if eq(fulfilledParts, 0xffffff) {
                mstore(0x00, 0x9a495418 /* TWAPNonceReuse() */ )
                revert(0x1c, 0x04)
            }

            sstore(bitmapPtr, or(flag, shl(MAX_TWAP_NONCE_SIZE, 0xffffff)))
        }
    }

    function _checkTWAPOrderData(uint32 interval, uint32 twapParts, uint32 window) internal pure {
        bool validInterval = interval < MIN_TWAP_INTERVAL || interval > MAX_TWAP_INTERVAL;
        bool validTParts = twapParts == 0 || twapParts > MAX_TWAP_TOTAL_PARTS;
        bool validWindow = window < MIN_TWAP_INTERVAL || window > interval;

        assembly("memory-safe") {
            if or(or(validInterval, validTParts), validWindow){
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

    function _invalidatePartTWAPNonceAndCheckDeadline(
        address owner, 
        uint64 nonce, 
        uint40 startTime, 
        uint32 interval, 
        uint32 twapParts,
        uint32 window
    ) 
        internal 
    {
        assembly ("memory-safe") {
            nonce := and(nonce, MASK_U64)
            mstore(12, div(nonce, MAX_TWAP_NONCE_SIZE))
            mstore(4, UNORDERED_TWAP_NONCES_SLOT)
            mstore(0, owner)

            let bitmapPtr := keccak256(12, 32)
            let flag := shl(mod(nonce, MAX_TWAP_NONCE_SIZE), 1)
            let bitmapVal := sload(bitmapPtr)
            let updated := xor(and(bitmapVal, MASK_U232), flag)
            let twapNonce := iszero(and(updated, flag))

            // part to fulfill
            let fulfilledParts := shr(MAX_TWAP_NONCE_SIZE, bitmapVal)
            let _cachedFulfilledParts := fulfilledParts

            // Reverts if `fulfilledParts` is empty while `twapNonce` is not empty,
            // or if `fulfilledParts` is not empty while `twapNonce` is empty.
            if xor(iszero(iszero(fulfilledParts)), twapNonce) {
                mstore(0x00, 0xcfa42043 /* InvalidTWAPNonce() */ )
                revert(0x1c, 0x04)
            }
            
            fulfilledParts := add(fulfilledParts, 1)
            twapParts:= and(twapParts, MASK_U32)

            if gt(fulfilledParts, twapParts) {
                mstore(0x00, 0x9a495418 /* TWAPNonceReuse() */ )
                revert(0x1c, 0x04)
            }

            updated := or(shl(MAX_TWAP_NONCE_SIZE, fulfilledParts), flag)

            if iszero(sub(twapParts, fulfilledParts)) {
                updated := or(updated, shl(MAX_TWAP_NONCE_SIZE, 0xffffff))
            }
            sstore(bitmapPtr, updated)

            let currentPartStart := add(and(startTime, MASK_U40), mul(_cachedFulfilledParts, and(interval, MASK_U32)))
            
            if or(lt(timestamp(), currentPartStart), gt(timestamp(), add(currentPartStart, and(window, MASK_U32)))) {
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
