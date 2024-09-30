// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import {console2 as console} from "forge-std/console2.sol";

struct TickRangeMap {
    uint256 size;
    int24[] keys;
    uint256[] values;
}

using TickRangeMapLib for TickRangeMap global;

/// @author philogy <https://github.com/philogy>
/// @dev The interface calls the key/values "ticks" & "indices".
library TickRangeMapLib {
    error NotSorted();
    error NotInRange(int24);
    error OutOfBounds(uint256, uint256);

    function add(TickRangeMap memory self, int24 key, uint256 value) internal pure {
        uint256 nextIndex = self.size++;
        uint256 len = self.keys.length;
        if (nextIndex >= len) {
            uint256 newLen = len > 0 ? len * 2 : 4;

            int24[] memory newKeys = new int24[](newLen);
            for (uint256 i = 0; i < len; i++) {
                newKeys[i] = self.keys[i];
            }
            self.keys = newKeys;

            uint256[] memory newValues = new uint256[](newLen);
            for (uint256 i = 0; i < len; i++) {
                newValues[i] = self.values[i];
            }
            self.values = newValues;
        }
        if (nextIndex > 0) {
            if (key <= self.keys[nextIndex - 1]) revert NotSorted();
            if (value <= self.values[nextIndex - 1]) revert NotSorted();
        }
        self.keys[nextIndex] = key;
        self.values[nextIndex] = value;
    }

    function brangeToTick(TickRangeMap memory self, uint256 ri) internal pure returns (int24) {
        return self.keys[self.size - ri - 1];
    }

    function brangeToIndex(TickRangeMap memory self, uint256 ri) internal pure returns (uint256) {
        console.log("self.size: %s", self.size);
        console.log("ri: %s", ri);
        return self.values[self.size - ri - 1];
    }

    function rangeToTick(TickRangeMap memory self, uint256 ri) internal pure returns (int24) {
        if (ri >= self.size) revert OutOfBounds(ri, self.size);
        return self.keys[ri];
    }

    function rangeToIndex(TickRangeMap memory self, uint256 ri)
        internal
        pure
        returns (uint256 index)
    {
        if (ri >= self.size) revert OutOfBounds(ri, self.size);
        index = self.values[ri];
    }

    function tickToRange(TickRangeMap memory self, int24 key)
        internal
        pure
        returns (bool inRange, uint256 ri)
    {
        uint256 size = self.size;
        for (; ri < size; ri++) {
            if (self.keys[ri] <= key) break;
        }
        inRange = ri + 1 < size;
        ri = ri == size ? 0 : ri;
    }
}
