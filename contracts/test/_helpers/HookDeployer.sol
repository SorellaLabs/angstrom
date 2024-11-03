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

    struct Create2Params {
        uint256 packedFactoryLeadByte;
        uint256 salt;
        bytes32 initcodeHash;
    }

    function deployHook(bytes memory initcode, uint160 flags, address factory)
        internal
        returns (bool success, address addr, bytes memory retdata)
    {
        Create2Params memory params = Create2Params(
            (uint256(0xff) << 160) | uint256(uint160(factory)), 0, keccak256(initcode)
        );

        while (true) {
            assembly ("memory-safe") {
                addr :=
                    and(keccak256(add(params, 11), 85), 0xffffffffffffffffffffffffffffffffffffffff)
            }
            if (uint160(addr) & Hooks.ALL_HOOK_MASK == flags) break;
            unchecked {
                params.salt++;
            }
        }

        (success, retdata) = factory.call(abi.encodePacked(params.salt, initcode));
        if (success) {
            assertEq(
                retdata,
                abi.encodePacked(addr),
                "Sanity check: factory returned data is not mined address"
            );
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
