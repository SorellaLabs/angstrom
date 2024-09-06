// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {Hooks} from "v4-core/src/libraries/Hooks.sol";
import {Test} from "forge-std/Test.sol";

import {console} from "forge-std/console.sol";

/// @author philogy <https://github.com/philogy>
abstract contract HookDeployer is Test {
    function _angstromFlags() internal pure returns (uint160) {
        return Hooks.BEFORE_SWAP_FLAG | Hooks.BEFORE_INITIALIZE_FLAG | Hooks.BEFORE_ADD_LIQUIDITY_FLAG
            | Hooks.AFTER_REMOVE_LIQUIDITY_FLAG;
    }

    function _newFactory() internal returns (address) {
        return address(new Create2Factory());
    }

    function deployHook(bytes memory initcode, uint160 flags, address factory)
        internal
        returns (bool success, address addr, bytes memory retdata)
    {
        bytes32 initcodeHash = keccak256(initcode);

        uint256 salt = 0;
        while (
            uint160(addr = vm.computeCreate2Address(bytes32(salt), initcodeHash, factory)) & Hooks.ALL_HOOK_MASK
                != flags
        ) {
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
