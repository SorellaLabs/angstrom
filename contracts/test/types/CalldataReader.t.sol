// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {Test} from "forge-std/Test.sol";
import {CalldataReader, CalldataReaderLib} from "../../src/types/CalldataReader.sol";

/// @author philogy <https://github.com/philogy>
contract CalladataReaderTest is Test {
    function test_fuzzing_readU8(bytes calldata garbage, uint8 x) public view {
        this._test_fuzzing_readU8(abi.encodePacked(x, garbage), x);
    }

    function _test_fuzzing_readU8(bytes calldata data, uint8 x) external pure {
        CalldataReader reader = CalldataReaderLib.from(data);
        (CalldataReader updatedReader, uint8 next) = CalldataReaderLib.from(data).readU8();
        assertEq(reader.offset() + 1, updatedReader.offset());
        assertEq(x, next);
        assertEq(bytes1(x), bytes1(data));
        uint256 full;
        assembly {
            full := next
        }
        assertEq(full, uint256(next) & 0xff);
    }

    function test_fuzzing_readU16(bytes calldata garbage, uint16 x) public view {
        this._test_fuzzing_readU16(abi.encodePacked(x, garbage), x);
    }

    function _test_fuzzing_readU16(bytes calldata data, uint16 x) external pure {
        CalldataReader reader = CalldataReaderLib.from(data);
        (CalldataReader updatedReader, uint16 next) = CalldataReaderLib.from(data).readU16();
        assertEq(reader.offset() + 2, updatedReader.offset());
        assertEq(x, next);
        assertEq(bytes2(x), bytes2(data));
        uint256 full;
        assembly {
            full := next
        }
        assertEq(full, uint256(next) & 0xffff);
    }

    function test_fuzzing_readU32(bytes calldata garbage, uint32 x) public view {
        this._test_fuzzing_readU32(abi.encodePacked(x, garbage), x);
    }

    function _test_fuzzing_readU32(bytes calldata data, uint32 x) external pure {
        CalldataReader reader = CalldataReaderLib.from(data);
        (CalldataReader updatedReader, uint32 next) = CalldataReaderLib.from(data).readU32();
        assertEq(reader.offset() + 4, updatedReader.offset());
        assertEq(x, next);
        assertEq(bytes4(x), bytes4(data));
        uint256 full;
        assembly {
            full := next
        }
        assertEq(full, uint256(next) & 0xffffffff);
    }

    function test_fuzzing_readU40(bytes calldata garbage, uint40 x) public view {
        this._test_fuzzing_readU40(abi.encodePacked(x, garbage), x);
    }

    function _test_fuzzing_readU40(bytes calldata data, uint40 x) external pure {
        CalldataReader reader = CalldataReaderLib.from(data);
        (CalldataReader updatedReader, uint40 next) = CalldataReaderLib.from(data).readU40();
        assertEq(reader.offset() + 5, updatedReader.offset());
        assertEq(x, next);
        assertEq(bytes5(uint40(x)), bytes5(data));
        uint256 full;
        assembly {
            full := next
        }
        assertEq(full, uint256(next) & 0xffffffffff);
    }

    function test_fuzzing_readU64(bytes calldata garbage, uint64 x) public view {
        this._test_fuzzing_readU64(abi.encodePacked(x, garbage), x);
    }

    function _test_fuzzing_readU64(bytes calldata data, uint64 x) external pure {
        CalldataReader reader = CalldataReaderLib.from(data);
        (CalldataReader updatedReader, uint64 next) = CalldataReaderLib.from(data).readU64();
        assertEq(reader.offset() + 8, updatedReader.offset());
        assertEq(x, next);
        assertEq(bytes8(x), bytes8(data));
        uint256 full;
        assembly {
            full := next
        }
        assertEq(full, uint256(next) & 0xffffffffffffffff);
    }

    function test_fuzzing_readU128(bytes calldata garbage, uint128 x) public view {
        this._test_fuzzing_readU128(abi.encodePacked(x, garbage), x);
    }

    function _test_fuzzing_readU128(bytes calldata data, uint128 x) external pure {
        CalldataReader reader = CalldataReaderLib.from(data);
        (CalldataReader updatedReader, uint128 next) = CalldataReaderLib.from(data).readU128();
        assertEq(reader.offset() + 16, updatedReader.offset());
        assertEq(x, next);
        assertEq(bytes16(x), bytes16(data));
        uint256 full;
        assembly {
            full := next
        }
        assertEq(full, uint256(next) & 0xffffffffffffffffffffffffffffffff);
    }

    function test_fuzzing_readAddr(bytes calldata garbage, address x) public view {
        this._test_fuzzing_readAddr(abi.encodePacked(x, garbage), x);
    }

    function _test_fuzzing_readAddr(bytes calldata data, address x) external pure {
        CalldataReader reader = CalldataReaderLib.from(data);
        (CalldataReader updatedReader, address next) = CalldataReaderLib.from(data).readAddr();
        assertEq(reader.offset() + 20, updatedReader.offset());
        assertEq(x, next);
        assertEq(bytes20(uint160(x)), bytes20(data));
        uint256 full;
        assembly {
            full := next
        }
        assertEq(full, uint256(uint160(next)));
    }

    function test_fuzzing_readI24(bytes calldata garbage, int24 x) public view {
        this._test_fuzzing_readI24(abi.encodePacked(x, garbage), x);
    }

    function _test_fuzzing_readI24(bytes calldata data, int24 x) external pure {
        CalldataReader reader = CalldataReaderLib.from(data);
        (CalldataReader updatedReader, int24 next) = CalldataReaderLib.from(data).readI24();
        assertEq(next, x);
        assertEq(reader.offset() + 3, updatedReader.offset());
        assertEq(bytes3(uint24(next)), bytes3(data));
        uint256 full;
        assembly {
            full := next
        }
        assertEq(full, uint256(int256(next)));
    }

    function test_fuzzing_readU256(bytes calldata garbage, uint256 x) public view {
        this._test_fuzzing_readU256(abi.encodePacked(x, garbage), x);
    }

    function _test_fuzzing_readU256(bytes calldata data, uint256 x) external pure {
        CalldataReader reader = CalldataReaderLib.from(data);
        (CalldataReader updatedReader, uint256 next) = reader.readU256();
        assertEq(next, x);
        assertEq(reader.offset() + 32, updatedReader.offset());
    }

    function test_fuzzing_readBytes(bytes calldata garbage, bytes calldata x) public view {
        vm.assume(x.length <= type(uint24).max);
        this._test_fuzzing_readBytes(abi.encodePacked(uint24(x.length), x, garbage), x);
    }

    function _test_fuzzing_readBytes(bytes calldata data, bytes calldata x) external pure {
        CalldataReader reader = CalldataReaderLib.from(data);
        (CalldataReader updatedReader, bytes calldata next) = reader.readBytes();
        assertEq(next, x);
        assertEq(reader.offset() + 3 + x.length, updatedReader.offset());
    }

    function test_fuzzing_from(bytes calldata data) public pure {
        CalldataReader reader = CalldataReaderLib.from(data);
        uint8 nextByte;
        for (uint256 i = 0; i < data.length; i++) {
            (reader, nextByte) = reader.readU8();
            assertEq(uint8(data[i]), nextByte);
        }
        reader.requireAtEndOf(data);
    }

    function test_fuzzing_requireAtEndOf_revertsIfNotAtEnd(bytes calldata data, uint256 bytesToRead) public {
        bytesToRead = (bound(bytesToRead, 1, data.length * 2 + 1) + data.length) % (data.length * 2 + 2);
        assertTrue(bytesToRead != data.length, "Failed to constrain bytes to read");
        CalldataReader reader = CalldataReaderLib.from(data);
        for (uint256 i = 0; i < bytesToRead; i++) {
            (reader,) = reader.readU8();
        }
        vm.expectRevert(CalldataReaderLib.ReaderNotAtEnd.selector);
        reader.requireAtEndOf(data);
    }
}
