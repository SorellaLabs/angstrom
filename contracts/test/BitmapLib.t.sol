// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {Test} from "forge-std/Test.sol";
import {BitmapLib} from "../src/libraries/BitmapLib.sol";

/// @author philogy <https://github.com/philogy>
contract BitmapLibTest is Test {
    function setUp() public {}

    function test_fuzzing_findNextGte(uint256 word, uint8 bitPos) public {
        (bool libInitialized, uint8 libPos) = BitmapLib.nextBitPosGte(word, bitPos);
        (uint8 cmpPos, bool cmpInitialized) = _findNextGte(word, bitPos);
        assertEq(libPos, cmpPos);
        assertEq(libInitialized, cmpInitialized);
    }

    function test_fuzzing_findNextLte(uint256 word, uint8 bitPos) public {
        (bool libInitialized, uint8 libPos) = BitmapLib.nextBitPosLte(word, bitPos);
        (uint8 cmpPos, bool cmpInitialized) = _findNextLte(word, bitPos);
        assertEq(libPos, cmpPos);
        assertEq(libInitialized, cmpInitialized);
    }

    function test_fuzzing_compress(int24 tick, int24 spacing) public {
        // Assumptin: Tick spacing is always a positive non-negative value.
        spacing = int24(bound(spacing, 1, type(int24).max));
        int24 libCompressed = BitmapLib.compress(tick, spacing);

        int24 safeCompressed = tick / spacing;
        if (tick < 0 && tick % spacing != 0) safeCompressed--;

        assertEq(libCompressed, safeCompressed);
    }

    function _findNextGte(uint256 word, uint8 bitPos) internal pure returns (uint8 nextBitPos, bool initialized) {
        for (uint256 i = bitPos; i < 256; i++) {
            if (word & (1 << i) != 0) return (uint8(i), true);
        }
        return (type(uint8).max, false);
    }

    function _findNextLte(uint256 word, uint8 bitPos) internal pure returns (uint8 nextBitPos, bool initialized) {
        while (true) {
            if (word & (1 << bitPos) != 0) return (uint8(bitPos), true);
            if (bitPos == 0) break;
            bitPos--;
        }
    }
}
