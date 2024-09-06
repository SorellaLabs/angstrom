// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";
import {InvalidationManager} from "src/modules/InvalidationManager.sol";
import {Utils} from "../_helpers/Utils.sol";

/// @author philogy <https://github.com/philogy>
contract InvalidationManagerTest is Test, InvalidationManager {
    using Utils for *;

    bytes4 internal constant NONCES_SLOT = bytes4(keccak256("angstrom-v1_0.unordered-nonces.slot"));

    function test_fuzzing_revertsUponReuse(address owner, uint64 nonce) public {
        _invalidateNonce(owner.brutalize(), nonce.brutalize());
        vm.expectRevert(InvalidationManager.NonceReuse.selector);
        _invalidateNonce(owner.brutalize(), nonce.brutalize());
    }
}
