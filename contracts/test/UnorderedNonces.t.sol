// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";
import {UnorderedNonces} from "../src/libraries/UnorderedNonces.sol";
import {Utils} from "./helpers/Utils.sol";

/// @author philogy <https://github.com/philogy>
contract UnorderedNoncesTest is Test, UnorderedNonces {
    using Utils for *;

    bytes4 internal constant NONCES_SLOT = bytes4(keccak256("angstrom-v1_0.unordered-nonces.slot"));

    function test_fuzzing_setDifferent(address owner, uint64 x, uint64 y) public {
        // Ensure `y` is not equal to `x` but still equally distributed.
        y = uint64(bound(y, 1, type(uint64).max) + x);
        _useNonce(owner.brutalize(), x.brutalize());
        _useNonce(owner.brutalize(), y.brutalize());

        if (x / 256 == y / 256) {
            uint256 expectedWord = (1 << (x & 0xff)) | (1 << (y & 0xff));
            assertEq(expectedWord, _getBitmapPtr(owner.brutalize(), x.brutalize()).load());
            bytes32 retrievedWord =
                vm.load(address(this), keccak256(abi.encodePacked(owner, NONCES_SLOT, uint56(x / 256))));
            assertEq(bytes32(expectedWord), retrievedWord);
        } else {
            uint256 xWord = 1 << (x & 0xff);
            assertEq(xWord, _getBitmapPtr(owner.brutalize(), x.brutalize()).load());
            bytes32 retrievedWord =
                vm.load(address(this), keccak256(abi.encodePacked(owner, NONCES_SLOT, uint56(x / 256))));
            assertEq(bytes32(xWord), retrievedWord);
            uint256 yWord = 1 << (y & 0xff);
            assertEq(yWord, _getBitmapPtr(owner.brutalize(), y.brutalize()).load());
            retrievedWord = vm.load(address(this), keccak256(abi.encodePacked(owner, NONCES_SLOT, uint56(y / 256))));
            assertEq(bytes32(yWord), retrievedWord);
        }
    }

    function test_fuzzing_revertsUponReuse(address owner, uint64 nonce) public {
        _useNonce(owner.brutalize(), nonce.brutalize());
        vm.expectRevert(UnorderedNonces.NonceReuse.selector);
        _useNonce(owner.brutalize(), nonce.brutalize());
    }
}
