// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import {CalldataReader} from "./CalldataReader.sol";
import {StructArrayLib} from "../libraries/StructArrayLib.sol";
import {AssetArray} from "./Asset.sol";
import {RayMathLib} from "../libraries/RayMathLib.sol";
import {FormatLib} from "super-sol/libraries/FormatLib.sol";

import {console} from "forge-std/console.sol";

type Pair is uint256;

type PairArray is uint256;

using PairLib for Pair global;
using PairLib for PairArray global;

/// @author philogy <https://github.com/philogy>
library PairLib {
    using FormatLib for *;
    using RayMathLib for uint256;
    using StructArrayLib for uint256;

    error OutOfOrderOrDuplicatePairs();
    error UnsortedPair();

    uint256 internal constant PAIR_BYTES = 36;

    uint256 internal constant INDEX_A_OFFSET = 0;
    uint256 internal constant INDEX_B_OFFSET = 2;
    uint256 internal constant PRICE_AB_OFFSET = 4;

    function readFromAndValidate(CalldataReader reader) internal pure returns (CalldataReader, PairArray) {
        uint256 packed;
        (reader, packed) = StructArrayLib.readPackedFrom(reader, PAIR_BYTES);
        return (reader, PairArray.wrap(packed)._validated());
    }

    function into(PairArray pairs) internal pure returns (uint256) {
        return PairArray.unwrap(pairs);
    }

    function into(Pair pair) internal pure returns (uint256) {
        return Pair.unwrap(pair);
    }

    function len(PairArray pairs) internal pure returns (uint256) {
        return pairs.into().len();
    }

    function _validated(PairArray self) internal pure returns (PairArray) {
        uint256 length = self.len();
        if (length == 0) return self;

        uint32 lastIndices = self.get(0).assetIndices();
        for (uint256 i = 1; i < length; i++) {
            uint32 indices = self.get(i).assetIndices();
            if (indices <= lastIndices) revert OutOfOrderOrDuplicatePairs();
            lastIndices = indices;
        }

        return self;
    }

    function get(PairArray self, uint256 index) internal pure returns (Pair asset) {
        self.into()._checkBounds(index);
        return Pair.wrap(self.into().ptr() + index * PAIR_BYTES);
    }

    function assetIndices(Pair self) internal pure returns (uint32 packedIndicies) {
        packedIndicies = self.into().readU32MemberFromPtr(INDEX_A_OFFSET);
    }

    function indexA(Pair self) internal pure returns (uint16 ia) {
        ia = self.into().readU16MemberFromPtr(INDEX_A_OFFSET);
    }

    function indexB(Pair self) internal pure returns (uint16 ib) {
        ib = self.into().readU16MemberFromPtr(INDEX_B_OFFSET);
    }

    function priceAB(Pair self) internal pure returns (uint256 priceAB_) {
        priceAB_ = self.into().readU256MemberFromPtr(PRICE_AB_OFFSET);
    }

    function decodeAndLookupPair(PairArray self, CalldataReader reader, AssetArray assets, bool aToB)
        internal
        pure
        returns (CalldataReader, address assetIn, address assetOut, uint256 priceOutVsIn)
    {
        uint256 pairIndex;
        (reader, pairIndex) = reader.readU16();
        Pair pair = self.get(pairIndex);
        uint256 pAB = pair.priceAB();

        address assetA = assets.get(pair.indexA()).addr();
        address assetB = assets.get(pair.indexB()).addr();

        (assetIn, assetOut, priceOutVsIn) = aToB ? (assetA, assetB, pAB.invRay()) : (assetB, assetA, pAB);

        return (reader, assetIn, assetOut, priceOutVsIn);
    }
}
