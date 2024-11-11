// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {LibBit} from "solady/src/utils/LibBit.sol";

/// @author philogy <https://github.com/philogy>
library TickLib {
    int24 internal constant MIN_TICK = -887272;
    int24 internal constant MAX_TICK = 887272;

    function isInitialized(uint256 word, uint8 bitPos) internal pure returns (bool) {
        return word & (uint256(1) << bitPos) != 0;
    }

    function nextBitPosLte(uint256 word, uint8 bitPos)
        internal
        pure
        returns (bool initialized, uint8 nextBitPos)
    {
        unchecked {
            uint8 offset = 0xff - bitPos;
            uint256 relativePos = LibBit.fls(word << offset);
            initialized = relativePos != 256;
            nextBitPos = initialized ? uint8(relativePos - offset) : 0;
        }
    }

    function nextBitPosGte(uint256 word, uint8 bitPos)
        internal
        pure
        returns (bool initialized, uint8 nextBitPos)
    {
        unchecked {
            uint256 relativePos = LibBit.ffs(word >> bitPos);
            initialized = relativePos != 256;
            nextBitPos = initialized ? uint8(relativePos + bitPos) : type(uint8).max;
        }
    }

    function compress(int24 tick, int24 tickSpacing) internal pure returns (int24 compressed) {
        assembly ("memory-safe") {
            compressed := sub(sdiv(tick, tickSpacing), slt(smod(tick, tickSpacing), 0))
        }
    }

    /// @dev Normalize tick to its tick boundary (rounding towards negative infinity). WARN: Can underflow
    /// for values of `tick < mul(sdiv(type(int24).min, tickSpacing), tickSpacing)`.
    function normalizeUnchecked(int24 tick, int24 tickSpacing)
        internal
        pure
        returns (int24 normalized)
    {
        assembly ("memory-safe") {
            normalized :=
                mul(sub(sdiv(tick, tickSpacing), slt(smod(tick, tickSpacing), 0)), tickSpacing)
        }
    }

    function position(int24 compressed) internal pure returns (int16 wordPos, uint8 bitPos) {
        assembly ("memory-safe") {
            wordPos := sar(8, compressed)
            bitPos := and(compressed, 0xff)
        }
    }

    function toTick(int16 wordPos, uint8 bitPos, int24 tickSpacing) internal pure returns (int24) {
        return (int24(wordPos) * 256 + int24(uint24(bitPos))) * tickSpacing;
    }
}
