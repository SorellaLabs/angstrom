// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

/// @dev Represents a calldata offset.
type CalldataReader is uint256;

using CalldataReaderLib for CalldataReader global;
using {neq as !=, eq as ==, gt as >, lt as <, ge as >=, le as <=} for CalldataReader global;

function neq(CalldataReader a, CalldataReader b) pure returns (bool) {
    return CalldataReader.unwrap(a) != CalldataReader.unwrap(b);
}

function eq(CalldataReader a, CalldataReader b) pure returns (bool) {
    return CalldataReader.unwrap(a) == CalldataReader.unwrap(b);
}

function gt(CalldataReader a, CalldataReader b) pure returns (bool) {
    return CalldataReader.unwrap(a) > CalldataReader.unwrap(b);
}

function lt(CalldataReader a, CalldataReader b) pure returns (bool) {
    return CalldataReader.unwrap(a) < CalldataReader.unwrap(b);
}

function ge(CalldataReader a, CalldataReader b) pure returns (bool) {
    return CalldataReader.unwrap(a) >= CalldataReader.unwrap(b);
}

function le(CalldataReader a, CalldataReader b) pure returns (bool) {
    return CalldataReader.unwrap(a) <= CalldataReader.unwrap(b);
}

/// @author philogy <https://github.com/philogy>
library CalldataReaderLib {
    error ReaderNotAtEnd();

    function from(bytes calldata data) internal pure returns (CalldataReader reader) {
        assembly {
            reader := data.offset
        }
    }

    function requireAtEndOf(CalldataReader self, bytes calldata data) internal pure {
        assembly ("memory-safe") {
            let end := add(data.offset, data.length)
            if iszero(eq(self, end)) {
                mstore(0x00, 0x01842f8c /* ReaderNotAtEnd() */ )
                revert(0x1c, 0x04)
            }
        }
    }

    function requireAtEndOf(CalldataReader self, CalldataReader end) internal pure {
        if (self != end) revert ReaderNotAtEnd();
    }

    function offset(CalldataReader self) internal pure returns (uint256) {
        return CalldataReader.unwrap(self);
    }

    function readBool(CalldataReader self) internal pure returns (CalldataReader, bool value) {
        assembly ("memory-safe") {
            value := gt(byte(0, calldataload(self)), 0)
            self := add(self, 1)
        }
        return (self, value);
    }

    function readU8(CalldataReader self) internal pure returns (CalldataReader, uint8 value) {
        assembly ("memory-safe") {
            value := byte(0, calldataload(self))
            self := add(self, 1)
        }
        return (self, value);
    }

    function readU16(CalldataReader self) internal pure returns (CalldataReader, uint16 value) {
        assembly ("memory-safe") {
            value := shr(240, calldataload(self))
            self := add(self, 2)
        }
        return (self, value);
    }

    function readU32(CalldataReader self) internal pure returns (CalldataReader, uint32 value) {
        assembly ("memory-safe") {
            value := shr(224, calldataload(self))
            self := add(self, 4)
        }
        return (self, value);
    }

    function readI24(CalldataReader self) internal pure returns (CalldataReader, int24 value) {
        assembly ("memory-safe") {
            value := sar(232, calldataload(self))
            self := add(self, 3)
        }
        return (self, value);
    }

    function readU40(CalldataReader self) internal pure returns (CalldataReader, uint40 value) {
        assembly ("memory-safe") {
            value := shr(216, calldataload(self))
            self := add(self, 5)
        }
        return (self, value);
    }

    function readU64(CalldataReader self) internal pure returns (CalldataReader, uint64 value) {
        assembly ("memory-safe") {
            value := shr(192, calldataload(self))
            self := add(self, 8)
        }
        return (self, value);
    }

    function readU128(CalldataReader self) internal pure returns (CalldataReader, uint128 value) {
        assembly ("memory-safe") {
            value := shr(128, calldataload(self))
            self := add(self, 16)
        }
        return (self, value);
    }

    function readAddr(CalldataReader self) internal pure returns (CalldataReader, address addr) {
        assembly ("memory-safe") {
            addr := shr(96, calldataload(self))
            self := add(self, 20)
        }
        return (self, addr);
    }

    function readU256(CalldataReader self) internal pure returns (CalldataReader, uint256 value) {
        assembly ("memory-safe") {
            value := calldataload(self)
            self := add(self, 32)
        }
        return (self, value);
    }

    function readU24End(CalldataReader self)
        internal
        pure
        returns (CalldataReader, CalldataReader end)
    {
        assembly ("memory-safe") {
            let len := shr(232, calldataload(self))
            self := add(self, 3)
            end := add(self, len)
        }
        return (self, end);
    }

    function readBytes(CalldataReader self)
        internal
        pure
        returns (CalldataReader, bytes calldata slice)
    {
        assembly ("memory-safe") {
            slice.length := shr(232, calldataload(self))
            self := add(self, 3)
            slice.offset := self
            self := add(self, slice.length)
        }
        return (self, slice);
    }
}
