// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import {CalldataReader} from "../types/CalldataReader.sol";
import {console} from "forge-std/console.sol";

/// @author philogy <https://github.com/philogy>
library StructArrayLib {
    using StructArrayLib for uint256;

    error OutOfBoundRead(uint256 arrayIndex, uint256 arrayLength);

    uint256 internal constant LENGTH_OFFSET = 32;
    uint256 internal constant CALLDATA_PTR_MASK = 0xffffffff;

    function readPackedFrom(CalldataReader reader, uint256 size)
        internal
        pure
        returns (CalldataReader, uint256 packed)
    {
        CalldataReader end;
        (reader, end) = reader.readU24End();

        // TODO: Full bytes could've not been used.
        uint256 length = (end.offset() - reader.offset()) / size;

        // For the offset to be more than 4 bytes we'd need at least 4 GB of calldata.
        packed = (length << LENGTH_OFFSET) | reader.offset();

        return (end, packed);
    }

    function len(uint256 packed) internal pure returns (uint256) {
        return packed >> LENGTH_OFFSET;
    }

    function ptr(uint256 packed) internal pure returns (uint256) {
        return packed & CALLDATA_PTR_MASK;
    }

    function readU16MemberFromPtr(uint256 cdPtr, uint256 memberOffset) internal pure returns (uint16 value) {
        assembly {
            value := shr(240, calldataload(add(cdPtr, memberOffset)))
        }
    }

    function readU32MemberFromPtr(uint256 cdPtr, uint256 memberOffset) internal pure returns (uint32 value) {
        assembly {
            value := shr(224, calldataload(add(cdPtr, memberOffset)))
        }
    }

    function readU128MemberFromPtr(uint256 cdPtr, uint256 memberOffset) internal pure returns (uint128 value) {
        assembly {
            value := shr(128, calldataload(add(cdPtr, memberOffset)))
        }
    }

    function readU256MemberFromPtr(uint256 cdPtr, uint256 memberOffset) internal pure returns (uint256 value) {
        assembly {
            value := calldataload(add(cdPtr, memberOffset))
        }
    }

    function readAddressMemberFromPtr(uint256 cdPtr, uint256 memberOffset) internal pure returns (address addr) {
        assembly {
            addr := shr(96, calldataload(add(cdPtr, memberOffset)))
        }
    }

    function _checkBounds(uint256 packed, uint256 index) internal pure {
        uint256 length = packed.len();
        if (index >= length) revert OutOfBoundRead(index, length);
    }
}
