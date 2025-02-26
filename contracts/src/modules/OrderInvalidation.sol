// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

/// @author philogy <https://github.com/philogy>
abstract contract OrderInvalidation {
    error NonceReuse();
    error OrderAlreadyExecuted();
    error Expired();
    error TWAPExpired();
    error InvalidTWAPOrder();
    error TWAPOrderNonceReuse();

    /// @dev `keccak256("angstrom-v1_0.unordered-nonces.slot")[0:4]`
    uint256 private constant UNORDERED_NONCES_SLOT = 0xdaa050e9;
    /// @dev `keccak256("angstrom-v1_0.twap-unordered-nonces.slot")[0:4]`
    uint256 private constant UNORDERED_TWAP_NONCES_SLOT = 0x635a0808;
    // type(uint24).max
    uint256 private constant MAX_U24 = 0xffffff;
    // type(uint32).max
    uint256 private constant MAX_U32 = 0xffffffff;
    // max upper limit of twap intervals = 31557600 (365.25 days)
    uint256 private constant MAX_TWAP_INTERVAL = 31557600;
    // min lower limit of twap intervals = 12 seconds
    uint256 private constant MIN_TWAP_INTERVAL = 12;
    // max no. of order parts = 6311520 (365.25 days / 5 seconds)
    uint256 private constant MAX_TWAP_TOTAL_PARTS = 6311520;

    function invalidateNonce(uint64 nonce) external {
        _invalidateNonce(msg.sender, nonce);
    }

    function invalidateTWAPOrderNonce(uint64 nonce) external {
        assembly ("memory-safe") {
            mstore(12, nonce)
            mstore(4, UNORDERED_TWAP_NONCES_SLOT)
            mstore(0, caller())

            let partPtr := keccak256(12, 32)
            let bitmap := sload(partPtr)

            if eq(and(bitmap, MAX_U24), MAX_U24) {
                mstore(0x00, 0x264a877f /* TWAPOrderNonceReuse() */ )
                revert(0x1c, 0x04)
            }

            sstore(partPtr, MAX_U24)
        }
    }

    function _checkTWAPOrderData(uint32 interval, uint32 twapParts, uint32 window) internal pure {
        bool invalidInterval = (interval < MIN_TWAP_INTERVAL) || (interval > MAX_TWAP_INTERVAL);
        bool invalidTwapParts = (twapParts == 0) || (twapParts > MAX_TWAP_TOTAL_PARTS);
        bool invalidWindow = (window < MIN_TWAP_INTERVAL) || (window > interval);

        if (invalidInterval || invalidTwapParts || invalidWindow) {
            revert InvalidTWAPOrder();
        }
    }

    function _checkTWAPOrderDeadline(
        uint256 fulfilledParts,
        uint40 startTime,
        uint32 interval,
        uint32 window
    ) internal view {
        uint256 currentPartStart = startTime + (fulfilledParts * interval);
        bool expired =
            (block.timestamp < currentPartStart) || (block.timestamp > currentPartStart + window);
        if (expired) revert TWAPExpired();
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

    function _invalidatePartTWAPNonce(
        bytes32 orderHash,
        address owner,
        uint256 nonce,
        uint32 twapParts
    ) internal returns (uint256 _cachedFulfilledParts) {
        uint256 bitmap;
        uint256 partPtr;
        assembly ("memory-safe") {
            mstore(12, nonce)
            mstore(4, UNORDERED_TWAP_NONCES_SLOT)
            mstore(0, owner)
            partPtr := keccak256(12, 32)

            // part to fulfill
            bitmap := sload(partPtr)

            // the probability that two order hashes collide in their lower 232 bits is 1 in 2^232.
            // for orders tied to a specific address, the space of possible values is more limited,
            // making the chance of collision even smaller.
            if iszero(bitmap) { bitmap := shl(24, orderHash) }
        }

        uint256 lowerHashBits = uint232(uint256(orderHash)) ^ bitmap >> 24;
        if (lowerHashBits != 0) revert TWAPOrderNonceReuse();

        _cachedFulfilledParts = bitmap & MAX_U24;
        uint256 fulfilledParts = _cachedFulfilledParts + 1;

        if (fulfilledParts != twapParts) {
            bitmap += 1;
        } else {
            bitmap = 0;
        }

        assembly ("memory-safe") {
            sstore(partPtr, bitmap)
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
