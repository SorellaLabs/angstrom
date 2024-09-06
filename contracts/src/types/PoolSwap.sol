// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import {CalldataReader} from "./CalldataReader.sol";
import {StructArrayLib} from "../libraries/StructArrayLib.sol";
import {AssetArray} from "./Asset.sol";

type PoolSwap is uint256;

using PoolSwapLib for PoolSwap global;

/// @author philogy <https://github.com/philogy>
library PoolSwapLib {
    using StructArrayLib for uint256;

    uint256 internal constant SWAP_BYTES = 20;

    uint256 internal constant INDEX_A_OFFSET = 0;
    uint256 internal constant INDEX_B_OFFSET = 2;
    uint256 internal constant AMOUNT_IN_OFFSET = 4;

    function readNextFrom(CalldataReader reader) internal pure returns (CalldataReader, PoolSwap swap) {
        assembly {
            swap := reader
            reader := add(reader, SWAP_BYTES)
        }
        return (reader, swap);
    }

    function into(PoolSwap self) internal pure returns (uint256) {
        return PoolSwap.unwrap(self);
    }

    function assetIndices(PoolSwap self) internal pure returns (uint32 packedIndices) {
        packedIndices = self.into().readU32MemberFromPtr(INDEX_A_OFFSET);
    }

    function indexA(PoolSwap self) internal pure returns (uint16 ia) {
        ia = self.into().readU16MemberFromPtr(INDEX_A_OFFSET);
    }

    function indexB(PoolSwap self) internal pure returns (uint16 ib) {
        ib = self.into().readU16MemberFromPtr(INDEX_B_OFFSET);
    }

    function amountIn(PoolSwap self) internal pure returns (uint256) {
        return self.into().readU128MemberFromPtr(AMOUNT_IN_OFFSET);
    }

    function getSwapAssets(PoolSwap self, AssetArray assets)
        internal
        pure
        returns (address asset0, address asset1, bool zeroForOne)
    {
        address assetIn = assets.get(self.indexA()).addr();
        address assetOut = assets.get(self.indexB()).addr();
        zeroForOne = assetIn < assetOut;
        (asset0, asset1) = zeroForOne ? (assetIn, assetOut) : (assetOut, assetIn);
    }
}
