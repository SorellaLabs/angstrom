// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

/// @author philogy <https://github.com/philogy>
library BitPackLib {
    error BitOverlap();

    /// @dev Equivalent of bitwise OR except it checks that no bits are overlapping
    function bitOverlay(uint256 x, uint256 y) internal pure returns (uint256) {
        if (x & y != 0) revert BitOverlap();
        return x | y;
    }
}
