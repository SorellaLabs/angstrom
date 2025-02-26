// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";
import {OrderInvalidation} from "src/modules/OrderInvalidation.sol";
import {Utils} from "../_helpers/Utils.sol";

/// @author philogy <https://github.com/philogy>
contract InvalidationManagerTest is Test, OrderInvalidation {
    using Utils for *;

    bytes4 internal constant NONCES_SLOT = bytes4(keccak256("angstrom-v1_0.unordered-nonces.slot"));
    uint256 private constant MAX_TWAP_INTERVAL = 31557600;
    uint256 private constant MIN_TWAP_INTERVAL = 12;
    uint256 private constant MAX_TWAP_TOTAL_PARTS = 6311520;
    uint256 private constant MAX_U32_VAL = type(uint32).max;

    function test_fuzzing_revertsUponReuse(address owner, uint64 nonce) public {
        _invalidateNonce(owner.brutalize(), nonce.brutalize());
        vm.expectRevert(OrderInvalidation.NonceReuse.selector);
        _invalidateNonce(owner.brutalize(), nonce.brutalize());
    }

    function test_fuzzing_revertsUponAlreadyExecutedOrder(uint64 nonce) public {
        this.invalidateTWAPOrderNonce(nonce.brutalize());
        vm.expectRevert(OrderInvalidation.TWAPOrderNonceReuse.selector);
        this.invalidateTWAPOrderNonce(nonce.brutalize());
    }

    function test_fuzzing_revertsUponExpiry(
        uint256 fulfilledParts,
        uint40 startTime,
        uint32 interval,
        uint32 window
    ) public {
        interval = uint32(bound(uint256(interval), MIN_TWAP_INTERVAL, MAX_TWAP_INTERVAL));
        window = uint32(bound(uint256(interval), MIN_TWAP_INTERVAL, interval));
        fulfilledParts = bound(fulfilledParts, 1, MAX_TWAP_TOTAL_PARTS);

        vm.warp(startTime + (interval * fulfilledParts));
        _checkTWAPOrderDeadline(fulfilledParts, startTime, interval, window);

        vm.warp(startTime + (interval * fulfilledParts) + window);
        _checkTWAPOrderDeadline(fulfilledParts, startTime, interval, window);

        vm.warp(startTime + (interval * fulfilledParts) - 1);
        vm.expectRevert(OrderInvalidation.TWAPExpired.selector);
        _checkTWAPOrderDeadline(fulfilledParts, startTime, interval, window);

        vm.warp(startTime + (interval * fulfilledParts) + window + 1);
        vm.expectRevert(OrderInvalidation.TWAPExpired.selector);
        _checkTWAPOrderDeadline(fulfilledParts, startTime, interval, window);
    }

    /// forge-config: default.allow_internal_expect_revert = true
    function test_fuzzing_revertsUponInvalidTWAPData(
        uint32 interval,
        uint32 twapParts,
        uint32 window
    ) public {
        interval = uint32(bound(uint256(interval), MIN_TWAP_INTERVAL, MAX_TWAP_INTERVAL));
        twapParts = uint32(bound(uint256(twapParts), 1, MAX_TWAP_TOTAL_PARTS));
        window = uint32(bound(uint256(interval), MIN_TWAP_INTERVAL, interval));
        _checkTWAPOrderData(interval, twapParts, window);

        interval = uint32(bound(uint256(interval), 0, MIN_TWAP_INTERVAL - 1));
        vm.expectRevert(OrderInvalidation.InvalidTWAPOrder.selector);
        _checkTWAPOrderData(interval, twapParts, window);

        interval = uint32(bound(uint256(interval), MIN_TWAP_INTERVAL + 1, MAX_U32_VAL));
        vm.expectRevert(OrderInvalidation.InvalidTWAPOrder.selector);
        _checkTWAPOrderData(interval, twapParts, window);

        interval = uint32(bound(uint256(interval), MIN_TWAP_INTERVAL, MAX_TWAP_INTERVAL));
        twapParts = uint32(bound(uint256(twapParts), MAX_TWAP_TOTAL_PARTS + 1, MAX_U32_VAL));
        vm.expectRevert(OrderInvalidation.InvalidTWAPOrder.selector);
        _checkTWAPOrderData(interval, twapParts, window);

        twapParts = 0;
        vm.expectRevert(OrderInvalidation.InvalidTWAPOrder.selector);
        _checkTWAPOrderData(interval, twapParts, window);

        twapParts = uint32(bound(uint256(twapParts), 1, MAX_TWAP_TOTAL_PARTS));
        window = uint32(bound(uint256(interval), interval + 1, MAX_U32_VAL));
        vm.expectRevert(OrderInvalidation.InvalidTWAPOrder.selector);
        _checkTWAPOrderData(interval, twapParts, window);

        window = uint32(bound(uint256(interval), 0, interval - 1));
        vm.expectRevert(OrderInvalidation.InvalidTWAPOrder.selector);
        _checkTWAPOrderData(interval, twapParts, window);
    }

    /// forge-config: default.allow_internal_expect_revert = true
    function test_fuzzing_revertsUponPartsResetTWAPNonce(
        bytes32 orderHash,
        address owner,
        uint64 nonce,
        uint32 twapParts
    ) public {
        twapParts = uint32(bound(uint256(twapParts), 0, 25));

        for (uint256 i = twapParts; i != 0; i--) {
            _invalidatePartTWAPNonce(
                orderHash, owner.brutalize(), nonce.brutalize(), twapParts.brutalizeU32()
            );
        }
        vm.expectRevert(OrderInvalidation.TWAPOrderNonceReuse.selector);
        _invalidatePartTWAPNonce(
            orderHash, owner.brutalize(), nonce.brutalize(), twapParts.brutalizeU32()
        );
    }
}
