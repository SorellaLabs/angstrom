// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {FixedPointMathLib} from "solady/src/utils/FixedPointMathLib.sol";

/// @author philogy <https://github.com/philogy>
/// @dev Similar to "wad math" except that the decimals used is a bit higher for the sake of
/// precision. Done to accommodate tokens that maybe have very large denominations.
library RayMathLib {
    using FixedPointMathLib for uint256;

    uint256 internal constant RAY = 1e27;
    uint256 internal constant RAY_2 = 1e54;

    function mulRayDown(uint256 x, uint256 y) internal pure returns (uint256) {
        return x * y / RAY;
    }

    function mulRayUp(uint256 x, uint256 y) internal pure returns (uint256) {
        return x.mulDivUp(y, RAY);
    }

    function divRayDown(uint256 x, uint256 y) internal pure returns (uint256) {
        return x * RAY / y;
    }

    function divRayUp(uint256 x, uint256 y) internal pure returns (uint256) {
        return x.mulDivUp(RAY, y);
    }

    function invRayUnchecked(uint256 x) internal pure returns (uint256 y) {
        assembly ("memory-safe") {
            y := div(RAY_2, x)
        }
    }
}
