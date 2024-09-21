// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {Hooks} from "v4-core/src/libraries/Hooks.sol";
import {Test} from "forge-std/Test.sol";

import {console} from "forge-std/console.sol";

/// @author philogy <https://github.com/philogy>
abstract contract HookDeployer is Test {
    function _newFactory() internal returns (address) {
        return address(new Create2Factory());
    }

    function deployHook(bytes memory initcode, uint160 flags, address factory)
        internal
        returns (bool success, address addr, bytes memory retdata)
    {
        bytes32 initcodeHash = keccak256(initcode);

        uint256 salt = 0;
        while (uint160(addr = computeCreate2(factory, salt, initcodeHash)) & Hooks.ALL_HOOK_MASK != flags) {
            salt++;
        }

        (success, retdata) = factory.call(abi.encodePacked(salt, initcode));
        if (success) {
            assertEq(retdata, abi.encodePacked(addr), "Sanity check: factory returned data is not mined address");
        } else {
            assembly ("memory-safe") {
                revert(add(retdata, 0x20), mload(retdata))
            }
        }
    }

    function computeCreate2(address factory, uint256 salt, bytes32 initcodeHash) internal pure returns (address) {
        return address(uint160(uint256(keccak256(abi.encodePacked(bytes1(0xff), factory, salt, initcodeHash)))));
    }
}

contract Create2Factory {
    fallback() external payable {
        _create();
    }

    function _create() internal {
        assembly {
            if iszero(gt(calldatasize(), 31)) { revert(0, 0) }
            let salt := calldataload(0x00)
            let size := sub(calldatasize(), 0x20)
            calldatacopy(0x00, 0x20, size)
            let result := create2(callvalue(), 0x00, size, salt)
            if iszero(result) {
                returndatacopy(0, 0, returndatasize())
                revert(0, returndatasize())
            }
            mstore(0, result)
            return(12, 20)
        }
    }
}
