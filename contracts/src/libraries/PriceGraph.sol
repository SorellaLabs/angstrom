// SPDX-License-Identifier: MIT
pragma solidity ^0.8.4;

import {Branchless} from "./Branchless.sol";
import {RayMathLib} from "./RayMathLib.sol";

/// @dev Packed `uint232(memOffset) || uint24(width)
type PriceGraph is uint256;

using PriceGraphLib for PriceGraph global;

type AssetIndex is uint16;

function into(AssetIndex index) pure returns (uint16) {
    return AssetIndex.unwrap(index);
}

using {into} for AssetIndex global;

/// @author philogy <https://github.com/philogy>
library PriceGraphLib {
    using RayMathLib for uint256;
    using Branchless for bool;

    error InvalidIndices(bool outOfOrder, bool outOfBounds, AssetIndex i, AssetIndex j);
    error AboveMaxWidth();

    error PriceNotSet();
    error PriceAlreadySet();

    uint256 internal constant MAX_WIDTH = 2000;

    uint256 private constant WIDTH_BIT_MASK = 0xffff;
    uint256 private constant MEM_PTR_BIT_OFFSET = 24;
    uint256 private constant CELL_SIZE = 33;

    function init(uint256 initWidth) internal pure returns (PriceGraph graph) {
        if (initWidth > MAX_WIDTH) revert AboveMaxWidth();
        uint256 cellCount = pyramidColumnOffset(initWidth);
        assembly ("memory-safe") {
            let memOffset := mload(0x40)
            // In our price matrix we have `w * (w - 1)` price cells with 33 bytes each (1 byte for
            // the "is set" flag, the other 32 for the `uint256` to store the actual price).
            let totalBytes := mul(CELL_SIZE, cellCount)
            // Clear memory region by filling with zeros.
            calldatacopy(memOffset, calldatasize(), totalBytes)
            mstore(0x40, add(memOffset, totalBytes))

            graph := or(shl(MEM_PTR_BIT_OFFSET, memOffset), initWidth)
        }
    }

    function width(PriceGraph self) internal pure returns (uint256) {
        return PriceGraph.unwrap(self) & WIDTH_BIT_MASK;
    }

    function set(PriceGraph self, AssetIndex i, AssetIndex j, uint256 price) internal pure {
        self._checkIndices(i, j);
        uint256 columnOffset = pyramidColumnOffset(i.into());
        assembly ("memory-safe") {
            let memOffset := shr(MEM_PTR_BIT_OFFSET, self)
            let cellOffset := add(memOffset, mul(CELL_SIZE, add(columnOffset, j)))
            if byte(0, mload(cellOffset)) {
                mstore(0x00, 0xfaa3eb24 /* PriceAlreadySet() */ )
                revert(0x1c, 0x04)
            }
            mstore8(cellOffset, true)
            mstore(add(cellOffset, 1), price)
        }
    }

    function get(PriceGraph self, AssetIndex i, AssetIndex j) internal pure returns (uint256 price) {
        return i.into() > j.into() ? _get(self, i, j) : _get(self, j, i).inv();
    }

    function _get(PriceGraph self, AssetIndex i, AssetIndex j) private pure returns (uint256 price) {
        self._checkIndices(i, j);
        uint256 columnOffset = pyramidColumnOffset(i.into());
        assembly ("memory-safe") {
            let memOffset := shr(MEM_PTR_BIT_OFFSET, self)
            let cellOffset := add(memOffset, mul(CELL_SIZE, add(columnOffset, j)))
            if iszero(byte(0, mload(cellOffset))) {
                mstore(0x00, 0x27515afa /* PriceNotSet() */ )
                revert(0x1c, 0x04)
            }
            price := mload(add(cellOffset, 1))
        }
    }

    function _checkIndices(PriceGraph self, AssetIndex i, AssetIndex j) internal pure {
        if ((i.into() <= j.into()).or(i.into() >= self.width())) {
            revert InvalidIndices(i.into() <= j.into(), i.into() >= self.width(), i, j);
        }
    }

    /**
     * @dev Computes sum of all numbers from 0 to `x` (exclusive).
     */
    function pyramidColumnOffset(uint256 x) internal pure returns (uint256) {
        unchecked {
            return x * (x - 1) / 2;
        }
    }
}
