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

    function test_fuzzing_revertsUponAlreadyExecutedOrder(bytes32 orderHash) public {
        this.invalidateTWAPOrder(orderHash);
        vm.expectRevert(OrderInvalidation.TWAPOrderAlreadyExecuted.selector);
        this.invalidateTWAPOrder(orderHash);
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
        _checkTWAPOrderData(
            interval.brutalizeU32(), twapParts.brutalizeU32(), window.brutalizeU32()
        );

        interval = uint32(bound(uint256(interval), 0, MIN_TWAP_INTERVAL - 1));
        vm.expectRevert(OrderInvalidation.InvalidTWAPOrder.selector);
        _checkTWAPOrderData(
            interval.brutalizeU32(), twapParts.brutalizeU32(), window.brutalizeU32()
        );

        interval = uint32(bound(uint256(interval), MIN_TWAP_INTERVAL + 1, MAX_U32_VAL));
        vm.expectRevert(OrderInvalidation.InvalidTWAPOrder.selector);
        _checkTWAPOrderData(
            interval.brutalizeU32(), twapParts.brutalizeU32(), window.brutalizeU32()
        );

        interval = uint32(bound(uint256(interval), MIN_TWAP_INTERVAL, MAX_TWAP_INTERVAL));
        twapParts = uint32(bound(uint256(twapParts), MAX_TWAP_TOTAL_PARTS + 1, MAX_U32_VAL));
        vm.expectRevert(OrderInvalidation.InvalidTWAPOrder.selector);
        _checkTWAPOrderData(
            interval.brutalizeU32(), twapParts.brutalizeU32(), window.brutalizeU32()
        );

        twapParts = 0;
        vm.expectRevert(OrderInvalidation.InvalidTWAPOrder.selector);
        _checkTWAPOrderData(
            interval.brutalizeU32(), twapParts.brutalizeU32(), window.brutalizeU32()
        );

        twapParts = uint32(bound(uint256(twapParts), 1, MAX_TWAP_TOTAL_PARTS));
        window = uint32(bound(uint256(interval), interval + 1, MAX_U32_VAL));
        vm.expectRevert(OrderInvalidation.InvalidTWAPOrder.selector);
        _checkTWAPOrderData(
            interval.brutalizeU32(), twapParts.brutalizeU32(), window.brutalizeU32()
        );

        window = uint32(bound(uint256(interval), 0, interval - 1));
        vm.expectRevert(OrderInvalidation.InvalidTWAPOrder.selector);
        _checkTWAPOrderData(
            interval.brutalizeU32(), twapParts.brutalizeU32(), window.brutalizeU32()
        );
    }

    /// forge-config: default.allow_internal_expect_revert = true
    function test_fuzzing_revertsUponPartsTWAPAlreadyExecuted(
        bytes32 orderHash,
        address owner,
        uint32 interval,
        uint32 twapParts,
        uint32 window
    ) public {
        uint40 startTime = uint40(block.timestamp);
        interval = uint32(bound(uint256(interval), MIN_TWAP_INTERVAL, MAX_TWAP_INTERVAL));
        twapParts = uint32(bound(uint256(twapParts), 0, 25));
        window = uint32(bound(uint256(window), MIN_TWAP_INTERVAL, interval));

        for (uint256 i = twapParts; i != 0; i--) {
            _invalidatePartTWAPAndCheckDeadline(
                _computeTWAPOrderSlot(orderHash, owner.brutalize()),
                startTime.brutalizeU40(),
                interval.brutalizeU32(),
                twapParts.brutalizeU32(),
                window.brutalizeU32()
            );
            uint256 warpedTime = startTime + ((twapParts - (i - 1)) * interval);
            vm.warp(warpedTime);
        }
        vm.expectRevert(OrderInvalidation.TWAPOrderAlreadyExecuted.selector);
        _invalidatePartTWAPAndCheckDeadline(
            _computeTWAPOrderSlot(orderHash, owner.brutalize()),
            startTime.brutalizeU40(),
            interval.brutalizeU32(),
            twapParts.brutalizeU32(),
            window.brutalizeU32()
        );
    }
}
