// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import {CalldataReader} from "./CalldataReader.sol";
import {AssetArray} from "./Asset.sol";
import {RayMathLib} from "../libraries/RayMathLib.sol";
import {
    PoolConfigStore, StoreKey, HASH_TO_STORE_KEY_SHIFT
} from "../libraries/PoolConfigStore.sol";
// TODO: Remove
import {FormatLib} from "super-sol/libraries/FormatLib.sol";
import {console} from "forge-std/console.sol";

type Pair is uint256;

/// @dev
type PairArray is uint256;

using PairLib for Pair global;
using PairLib for PairArray global;

/// @author philogy <https://github.com/philogy>
library PairLib {
    // TODO: Remove
    using FormatLib for *;

    using RayMathLib for uint256;

    error OutOfOrderOrDuplicatePairs();
    error PairAccessOutOfBounds(uint256 index, uint256 length);

    uint256 internal constant PAIR_ARRAY_MEM_OFFSET_OFFSET = 32;
    uint256 internal constant PAIR_ARRAY_LENGTH_MASK = 0xffffffff;

    /// @dev 6 words for: asset0, asset1, priceAB, priceBA, tickSpacing, fee
    uint256 internal constant PAIR_MEM_BYTES = 0xc0;

    uint256 internal constant PAIR_ASSET0_OFFSET = 0x00;
    uint256 internal constant PAIR_ASSET1_OFFSET = 0x20;
    uint256 internal constant PAIR_TICK_SPACING_OFFSET = 0x40;
    uint256 internal constant PAIR_FEE_OFFSET = 0x60;
    uint256 internal constant PAIR_PRICE_10_OFFSET = 0x80;
    uint256 internal constant PAIR_PRICE_01_OFFSET = 0xa0;

    uint256 internal constant INDEX_A_OFFSET = 16;
    uint256 internal constant INDEX_B_MASK = 0xffff;

    uint256 internal constant PAIR_CD_BYTES = 38;

    uint256 internal constant ONE_E6 = 1e6;

    function readFromAndValidate(CalldataReader reader, AssetArray assets, PoolConfigStore store)
        internal
        view
        returns (CalldataReader, PairArray pairs)
    {
        uint256 raw_memoryOffset;
        uint256 raw_memoryEnd;

        CalldataReader end;
        {
            (reader, end) = reader.readU24End();
            uint256 length = (end.offset() - reader.offset()) / PAIR_CD_BYTES;

            assembly ("memory-safe") {
                // WARN: Memory is allocated, but **not cleaned**.
                raw_memoryOffset := mload(0x40)
                raw_memoryEnd := add(raw_memoryOffset, mul(PAIR_MEM_BYTES, length))
                mstore(0x40, raw_memoryEnd)
                // No need to mask length because we know it's less than 4 bytes (u24.max / PAIR_BYTES < u32.max)
                pairs := or(shl(PAIR_ARRAY_MEM_OFFSET_OFFSET, raw_memoryOffset), length)
            }
        }

        uint32 lastIndices;
        for (; raw_memoryOffset < raw_memoryEnd;) {
            // Load, decode and validate assets of pair.
            {
                uint32 indices;
                (reader, indices) = reader.readU32();
                address asset0 = assets.get(indices >> INDEX_A_OFFSET).addr();
                address asset1 = assets.get(indices & INDEX_B_MASK).addr();
                // We ensure pair uniqueness by ensuring that the list is sorted and that every pair
                // is unique by ensuring there's only one valid ordering of asset 0 & 1.
                if (indices <= lastIndices || asset0 >= asset1) revert OutOfOrderOrDuplicatePairs();
                lastIndices = indices;

                assembly ("memory-safe") {
                    mstore(add(raw_memoryOffset, PAIR_ASSET0_OFFSET), asset0)
                    mstore(add(raw_memoryOffset, PAIR_ASSET1_OFFSET), asset1)
                }
            }

            // Load and store pool config.
            {
                StoreKey key;
                assembly ("memory-safe") {
                    key :=
                        shl(
                            HASH_TO_STORE_KEY_SHIFT,
                            keccak256(add(raw_memoryOffset, PAIR_ASSET0_OFFSET), 0x40)
                        )
                }

                uint16 storeIndex;
                (reader, storeIndex) = reader.readU16();
                (int24 tickSpacing, uint24 feeIne6) = store.get(key, storeIndex);

                assembly ("memory-safe") {
                    mstore(add(raw_memoryOffset, PAIR_TICK_SPACING_OFFSET), tickSpacing)
                    mstore(add(raw_memoryOffset, PAIR_FEE_OFFSET), feeIne6)
                }
            }

            // Load main AB price, compute inverse, store both.
            {
                uint256 price1Over0;
                (reader, price1Over0) = reader.readU256();
                uint256 price0Over1 = price1Over0.invRayUnchecked();
                assembly ("memory-safe") {
                    mstore(add(raw_memoryOffset, PAIR_PRICE_10_OFFSET), price1Over0)
                    mstore(add(raw_memoryOffset, PAIR_PRICE_01_OFFSET), price0Over1)
                }
            }

            unchecked {
                raw_memoryOffset += PAIR_MEM_BYTES;
            }
        }

        return (end, pairs);
    }

    function len(PairArray self) internal pure returns (uint256 length) {
        return PairArray.unwrap(self) & PAIR_ARRAY_LENGTH_MASK;
    }

    function get(PairArray self, uint256 index) internal pure returns (Pair pair) {
        if (self.len() <= index) revert PairAccessOutOfBounds(index, self.len());
        uint256 raw_memoryOffset = PairArray.unwrap(self) >> PAIR_ARRAY_MEM_OFFSET_OFFSET;
        unchecked {
            return Pair.wrap(raw_memoryOffset + index * PAIR_MEM_BYTES);
        }
    }

    function getPoolInfo(Pair self)
        internal
        pure
        returns (address asset0, address asset1, int24 tickSpacing)
    {
        assembly ("memory-safe") {
            asset0 := mload(add(self, PAIR_ASSET0_OFFSET))
            asset1 := mload(add(self, PAIR_ASSET1_OFFSET))
            tickSpacing := mload(add(self, PAIR_TICK_SPACING_OFFSET))
        }
    }

    function getAssets(Pair self, bool zeroToOne)
        internal
        pure
        returns (address assetIn, address assetOut)
    {
        assembly ("memory-safe") {
            let offsetIfZeroToOne := shl(5, zeroToOne)
            assetIn := mload(add(self, xor(offsetIfZeroToOne, 0x20)))
            assetOut := mload(add(self, offsetIfZeroToOne))
        }
    }

    function getSwapInfo(Pair self, bool zeroToOne)
        internal
        pure
        returns (address assetIn, address assetOut, uint256 priceOutVsIn)
    {
        uint256 oneMinusFee;
        assembly ("memory-safe") {
            let offsetIfZeroToOne := shl(5, zeroToOne)
            assetIn := mload(add(self, xor(offsetIfZeroToOne, 0x20)))
            assetOut := mload(add(self, offsetIfZeroToOne))
            priceOutVsIn := mload(add(self, add(PAIR_PRICE_10_OFFSET, offsetIfZeroToOne)))
            oneMinusFee := sub(ONE_E6, mload(add(self, PAIR_FEE_OFFSET)))
        }
        priceOutVsIn = priceOutVsIn * oneMinusFee / ONE_E6;
    }
}
