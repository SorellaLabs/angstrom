// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

/// @author philogy <https://github.com/philogy>
abstract contract OrderInvalidation {
    error NonceReuse();
    error OrderAlreadyExecuted();
    error Expired();
    error TWAPOrderAlreadyExecuted();
    error TWAPExpired();
    error InvalidTWAPOrder();

    /// @dev `keccak256("angstrom-v1_0.unordered-nonces.slot")[0:4]`
    uint256 private constant UNORDERED_NONCES_SLOT = 0xdaa050e9;

    // type(uint32).max
    uint256 private constant MASK_U32 = 0xffffffff;
    // type(uint40).max
    uint256 private constant MASK_U40 = 0xffffffffff;

    // max upper limit of twap intervals = 31557600 (365.25 days)
    uint256 private constant MAX_TWAP_INTERVAL = 31557600;
    // min lower limit of twap intervals = 12 seconds
    uint256 private constant MIN_TWAP_INTERVAL = 12;
    // max no. of order parts = 6311520 (365.25 days / 5 seconds)
    uint256 private constant MAX_TWAP_TOTAL_PARTS = 6311520;

    function invalidateNonce(uint64 nonce) external {
        _invalidateNonce(msg.sender, nonce);
    }

    function invalidateTWAPOrder(bytes32 orderHash) external {
        assembly ("memory-safe") {
            mstore(20, caller())
            mstore(0, orderHash)
            let partPtr := keccak256(0, 52)

            let fulfilledParts := sload(partPtr)

            if eq(fulfilledParts, 0xffffff) {
                mstore(0x00, 0xceb2041c /* TWAPOrderAlreadyExecuted() */ )
                revert(0x1c, 0x04)
            }

            sstore(partPtr, 0xffffff)
        }
    }

    function _checkTWAPOrderData(uint32 interval, uint32 twapParts, uint32 window) internal pure {
        assembly ("memory-safe") {
            interval := and(interval, MASK_U32)
            twapParts := and(twapParts, MASK_U32)
            window := and(window, MASK_U32)

            let validInterval :=
                or(lt(interval, MIN_TWAP_INTERVAL), gt(interval, MAX_TWAP_INTERVAL))
            let validTwapParts := or(iszero(twapParts), gt(twapParts, MAX_TWAP_TOTAL_PARTS))
            let validWindow := or(lt(window, MIN_TWAP_INTERVAL), gt(window, interval))

            if or(or(validInterval, validTwapParts), validWindow) {
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

    function _computeTWAPOrderSlot(bytes32 orderHash, address owner)
        internal
        pure
        returns (bytes32 partPtr)
    {
        assembly ("memory-safe") {
            mstore(20, owner)
            mstore(0, orderHash)
            partPtr := keccak256(0, 52)
        }
    }

    function _invalidatePartTWAPAndCheckDeadline(
        bytes32 partPtr,
        uint40 startTime,
        uint32 interval,
        uint32 twapParts,
        uint32 window
    ) internal {
        assembly ("memory-safe") {
            // part to fulfill
            let fulfilledParts := sload(partPtr)
            let _cachedFulfilledParts := fulfilledParts

            fulfilledParts := add(fulfilledParts, 1)
            twapParts := and(twapParts, MASK_U32)

            if gt(fulfilledParts, twapParts) {
                mstore(0x00, 0xceb2041c /* TWAPOrderAlreadyExecuted() */ )
                revert(0x1c, 0x04)
            }

            if eq(twapParts, fulfilledParts) { fulfilledParts := 0xffffff }
            sstore(partPtr, fulfilledParts)

            let currentPartStart :=
                add(and(startTime, MASK_U40), mul(_cachedFulfilledParts, and(interval, MASK_U32)))

            if or(
                lt(timestamp(), currentPartStart),
                gt(timestamp(), add(currentPartStart, and(window, MASK_U32)))
            ) {
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
