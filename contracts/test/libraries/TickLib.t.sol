// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {Test} from "forge-std/Test.sol";
import {TickLib} from "../../src/libraries/TickLib.sol";
import {TickMath} from "v4-core/src/libraries/TickMath.sol";

/// @author philogy <https://github.com/philogy>
contract TickLibTest is Test {
    function setUp() public {}

    function test_findNextGte() public pure {
        assertNextBitPosGteEq(1, 0, true, 0);
        assertNextBitPosGteEq(1, 1, false, 255);
        assertNextBitPosGteEq(0x0201, 0, true, 0);
        assertNextBitPosGteEq(0x0201, 1, true, 9);
        assertNextBitPosGteEq(0x0201, 10, false, 255);
    }

    function test_fuzzing_findNextGte_zeroWord(uint8 bitPos) public pure {
        (bool initialized, uint8 outPos) = TickLib.nextBitPosGte(0, bitPos);
        assertFalse(initialized);
        assertEq(outPos, 255);
    }

    function test_fuzzing_findNextGte(uint256 word, uint8 bitPos) public pure {
        (bool libInitialized, uint8 libPos) = TickLib.nextBitPosGte(word, bitPos);
        (uint8 cmpPos, bool cmpInitialized) = _findNextGte(word, bitPos);
        assertEq(libPos, cmpPos);
        assertEq(libInitialized, cmpInitialized);
    }

    function test_fuzzing_findNextLte(uint256 word, uint8 bitPos) public pure {
        (bool libInitialized, uint8 libPos) = TickLib.nextBitPosLte(word, bitPos);
        (uint8 cmpPos, bool cmpInitialized) = _findNextLte(word, bitPos);
        assertEq(libPos, cmpPos);
        assertEq(libInitialized, cmpInitialized);
    }

    function test_fuzzing_compress(int24 tick, int24 tickSpacing) public pure {
        tickSpacing =
            int24(bound(tickSpacing, TickMath.MIN_TICK_SPACING, TickMath.MAX_TICK_SPACING));
        // Assumption: Tick spacing is always a positive non-negative value.
        int24 libCompressed = TickLib.compress(tick, tickSpacing);

        int24 safeCompressed = tick / tickSpacing;
        if (tick < 0 && tick % tickSpacing != 0) safeCompressed--;

        assertEq(libCompressed, safeCompressed);
    }

    function test_fuzzing_tickRecreatedFromPositionToTick(int24 tick, int24 tickSpacing)
        public
        pure
    {
        tickSpacing =
            int24(bound(tickSpacing, TickMath.MIN_TICK_SPACING, TickMath.MAX_TICK_SPACING));
        (int16 wordPos, uint8 bitPos) = TickLib.position(tick / tickSpacing);
        int24 outTick = TickLib.toTick(wordPos, bitPos, tickSpacing);
        assertEq(tick - tick % tickSpacing, outTick);
    }

    function _findNextGte(uint256 word, uint8 bitPos)
        internal
        pure
        returns (uint8 nextBitPos, bool initialized)
    {
        for (uint256 i = bitPos; i < 256; i++) {
            if (word & (1 << i) != 0) return (uint8(i), true);
        }
        return (type(uint8).max, false);
    }

    function _findNextLte(uint256 word, uint8 bitPos)
        internal
        pure
        returns (uint8 nextBitPos, bool initialized)
    {
        while (true) {
            if (word & (1 << bitPos) != 0) return (uint8(bitPos), true);
            if (bitPos == 0) break;
            bitPos--;
        }
    }

    function assertNextBitPosGteEq(
        uint256 word,
        uint8 bitPos,
        bool expectedInitialized,
        uint8 expectedOutBitPos
    ) internal pure {
        (bool initialized, uint8 outPos) = TickLib.nextBitPosGte(word, bitPos);
        assertEq(initialized, expectedInitialized);
        assertEq(outPos, expectedOutBitPos);
    }
}
