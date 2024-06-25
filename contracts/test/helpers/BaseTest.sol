// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";
import {console2 as console} from "forge-std/console2.sol";

/// @author philogy <https://github.com/philogy>
contract BaseTest is Test {
    function sign(Account memory account, bytes32 hash) internal pure returns (bytes memory) {
        console.log("%s signing %x", account.addr, uint256(hash));
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(account.key, hash);
        // Compress.
        uint256 vs = (uint256(v - 27) << 255) | uint256(s);
        return abi.encodePacked(r, vs);
    }
}
