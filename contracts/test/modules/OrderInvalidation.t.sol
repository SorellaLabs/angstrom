// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";
import {OrderInvalidation} from "src/modules/OrderInvalidation.sol";
import {Utils} from "../_helpers/Utils.sol";

/// @author philogy <https://github.com/philogy>
contract InvalidationManagerTest is Test, OrderInvalidation {
    using Utils for *;

    bytes4 internal constant NONCES_SLOT = bytes4(keccak256("angstrom-v1_0.unordered-nonces.slot"));
    uint256 private constant MAX_TWAP_INTERVAL = 0x1e187e0;
    uint256 private constant MAX_TWAP_TOTAL_PARTS = 0x604e60;

    function test_fuzzing_revertsUponReuse(address owner, uint64 nonce) public {
        _invalidateNonce(owner.brutalize(), nonce.brutalize());
        vm.expectRevert(OrderInvalidation.NonceReuse.selector);
        _invalidateNonce(owner.brutalize(), nonce.brutalize());
    }

    function test_fuzzing_revertsUponTWAPNonceReuse(uint64 nonce) public {
        this.invalidateTWAPNonce(nonce.brutalize());
        vm.expectRevert(OrderInvalidation.TWAPNonceReuse.selector);
        this.invalidateTWAPNonce(nonce.brutalize());
    }

    function test_fuzzing_revertsUponInvalidTWAPData(uint32 interval, uint32 tParts) public view {
        interval = uint32(bound(uint256(interval), 1, MAX_TWAP_INTERVAL));
        tParts = uint32(bound(uint256(tParts), 1, MAX_TWAP_TOTAL_PARTS));
        _checkTWAPOrderData(interval.brutalizeU32(), tParts.brutalizeU32());
    }

    /// forge-config: default.allow_internal_expect_revert = true
    function test_fuzzing_revertsUponPartsTWAPNonceReuse(
        address owner,
        uint64 nonce,
        uint32 interval, 
        uint32 tParts
    ) public {
        uint40 sTime = uint40(block.timestamp);
        interval = uint32(bound(uint256(interval), 0, MAX_TWAP_INTERVAL));
        tParts = uint32(bound(uint256(tParts), 0, 20));

        for(uint256 i = tParts; i != 0; i--){
            _invalidatePartTWAPNonceAndCheckDeadline(
                owner.brutalize(), 
                nonce.brutalize(), 
                sTime.brutalizeU40(), 
                interval.brutalizeU32(), 
                tParts.brutalizeU32()
            );
            uint256 warpedTime = sTime + ((tParts-(i-1)) * interval);
            vm.warp(warpedTime);
        }
        vm.expectRevert(OrderInvalidation.TWAPNonceReuse.selector);
        _invalidatePartTWAPNonceAndCheckDeadline(
            owner.brutalize(), 
            nonce.brutalize(), 
            sTime.brutalizeU40(), 
            interval.brutalizeU32(), 
            tParts.brutalizeU32()
        );
    }
}
