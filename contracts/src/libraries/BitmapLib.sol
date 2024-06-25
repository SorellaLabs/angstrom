// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {LibBit} from "solady/src/utils/LibBit.sol";

/// @author philogy <https://github.com/philogy>
library BitmapLib {
    function nextBitPosLte(uint256 word, uint8 bitPos) internal pure returns (bool initialized, uint8 nextBitPos) {
        unchecked {
            uint8 offset = 0xff - bitPos;
            uint256 relativePos = LibBit.fls(word << offset);
            initialized = relativePos != 256;
            nextBitPos = initialized ? uint8(relativePos - offset) : 0;
        }
    }

    function nextBitPosDown(uint256 word) internal pure returns (bool initialized, uint8 nextBitPos) {
        uint256 pos = LibBit.fls(word);
        initialized = pos != 256;
        nextBitPos = uint8(pos);
    }

    function nextBitPosGte(uint256 word, uint8 bitPos) internal pure returns (bool initialized, uint8 nextBitPos) {
        unchecked {
            uint256 relativePos = LibBit.ffs(word >> bitPos);
            initialized = relativePos != 256;
            nextBitPos = initialized ? uint8(relativePos + bitPos) : type(uint8).max;
        }
    }

    function nextBitPosUp(uint256 word) internal pure returns (bool initialized, uint8 nextBitPos) {
        uint256 pos = LibBit.ffs(word);
        initialized = pos != 256;
        nextBitPos = uint8(pos);
    }

    function compress(int24 tick, int24 spacing) internal pure returns (int24 compressed) {
        assembly {
            compressed := sub(sdiv(tick, spacing), slt(smod(tick, spacing), 0))
        }
    }

    function position(int24 compressed) internal pure returns (int16 wordPos, uint8 bitPos) {
        unchecked {
            wordPos = int16(compressed >> 8);
            bitPos = uint8(int8(compressed));
        }
    }
}
