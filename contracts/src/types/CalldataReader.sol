// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import {OrderVariantMap} from "./OrderVariantMap.sol";

import {console} from "forge-std/console.sol";
import {FormatLib} from "super-sol/libraries/FormatLib.sol";
import {TRACE_LOGS} from "../modules/DevFlags.sol";
import {LibString} from "solady/src/utils/LibString.sol";

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
    using FormatLib for *;
    using LibString for *;

    error ReaderNotAtEnd();
    error ReaderNotAtEndStrOver(string str, bool wentOver);

    function from(bytes calldata data) internal pure returns (CalldataReader reader) {
        assembly {
            reader := data.offset
        }
    }

    function requireAtEndOf(CalldataReader self, bytes calldata data) internal pure {
        assembly {
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

    function requireAtEndOf(CalldataReader self, CalldataReader end, bytes32 message) internal pure {
        if (self != end) {
            revert ReaderNotAtEndStrOver(LibString.fromSmallString(message), self > end);
        }
    }

    function offset(CalldataReader self) internal pure returns (uint256) {
        return CalldataReader.unwrap(self);
    }

    function readU8(CalldataReader self) internal pure returns (CalldataReader, uint8 value) {
        if (TRACE_LOGS) self.logPos(1, "u8");
        assembly {
            value := byte(0, calldataload(self))
            self := add(self, 1)
        }
        return (self, value);
    }

    function readU16(CalldataReader self) internal pure returns (CalldataReader, uint16 value) {
        if (TRACE_LOGS) self.logPos(2, "u16");
        assembly {
            value := shr(240, calldataload(self))
            self := add(self, 2)
        }
        return (self, value);
    }

    function readU32(CalldataReader self) internal pure returns (CalldataReader, uint32 value) {
        if (TRACE_LOGS) self.logPos(4, "u32");
        assembly {
            value := shr(224, calldataload(self))
            self := add(self, 4)
        }
        return (self, value);
    }

    function readI24(CalldataReader self) internal pure returns (CalldataReader, int24 value) {
        if (TRACE_LOGS) self.logPos(3, "i24");
        assembly {
            value := sar(232, calldataload(self))
            self := add(self, 3)
        }
        return (self, value);
    }

    function readU40(CalldataReader self) internal pure returns (CalldataReader, uint40 value) {
        if (TRACE_LOGS) self.logPos(5, "u40");
        assembly {
            value := shr(216, calldataload(self))
            self := add(self, 5)
        }
        return (self, value);
    }

    function readU64(CalldataReader self) internal pure returns (CalldataReader, uint64 value) {
        if (TRACE_LOGS) self.logPos(8, "u64");
        assembly {
            value := shr(192, calldataload(self))
            self := add(self, 8)
        }
        return (self, value);
    }

    function readU128(CalldataReader self) internal pure returns (CalldataReader, uint128 value) {
        if (TRACE_LOGS) self.logPos(16, "u128");
        assembly {
            value := shr(128, calldataload(self))
            self := add(self, 16)
        }
        return (self, value);
    }

    function readAddr(CalldataReader self) internal pure returns (CalldataReader, address addr) {
        if (TRACE_LOGS) self.logPos(20, "address");
        assembly {
            addr := shr(96, calldataload(self))
            self := add(self, 20)
        }
        return (self, addr);
    }

    function readU256(CalldataReader self) internal pure returns (CalldataReader, uint256 value) {
        if (TRACE_LOGS) self.logPos(32, "u256");
        assembly {
            value := calldataload(self)
            self := add(self, 32)
        }
        return (self, value);
    }

    function readVariant(CalldataReader self) internal pure returns (CalldataReader, OrderVariantMap variant) {
        if (TRACE_LOGS) self.logPos(1, "OrderVariantMap");
        assembly {
            variant := shr(248, calldataload(self))
            self := add(self, 1)
        }
        return (self, variant);
    }

    function readU24End(CalldataReader self) internal pure returns (CalldataReader, CalldataReader end) {
        if (TRACE_LOGS) self.logPos(3, "u24/List.length");
        assembly ("memory-safe") {
            let len := shr(232, calldataload(self))
            self := add(self, 3)
            end := add(self, len)
        }
        return (self, end);
    }

    function readBytes(CalldataReader self) internal pure returns (CalldataReader, bytes calldata slice) {
        if (TRACE_LOGS) {
            self.logPos(3, "u24/bytes.length");
            (CalldataReader postReader, bytes calldata b) = _readBytes(self);
            console.log(
                "[CalldataReader] reading next %s byte(s) at %s as bytes ->",
                b.length.toStr().lpad(" ", 6),
                postReader.offset().toHexString(3)
            );
            console.logBytes(b);
        }
        return _readBytes(self);
    }

    function _readBytes(CalldataReader self) private pure returns (CalldataReader, bytes calldata slice) {
        assembly ("memory-safe") {
            slice.length := shr(232, calldataload(self))
            self := add(self, 3)
            slice.offset := self
            self := add(self, slice.length)
        }
        return (self, slice);
    }

    function logNext(CalldataReader self, uint256 n) internal pure {
        bytes memory b;
        assembly ("memory-safe") {
            b := mload(0x40)
            mstore(b, n)
            let dataOffset := add(b, 0x20)
            mstore(0x40, add(dataOffset, n))
            calldatacopy(dataOffset, self, n)
        }
        console.logBytes(b);
    }

    function logPos(CalldataReader self, uint8 reading, string memory dtype) internal pure {
        uint256 read;
        assembly {
            read := shr(sub(256, mul(reading, 8)), calldataload(self))
        }
        console.log(
            string.concat(
                "[CalldataReader] reading next ",
                uint256(reading).toStr().lpad(" ", 6),
                " byte(s) at ",
                self.offset().toHexString(3),
                " as %s -> %s"
            ),
            dtype,
            read.toHexString(reading)
        );
    }
}
