// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {SafeCastLib} from "solady/src/utils/SafeCastLib.sol";
import {Asset, AssetLib} from "./Asset.sol";
import {RayMathLib} from "../libraries/RayMathLib.sol";
import {PairLib as ActualPairLib} from "../types/Pair.sol";
import {PriceAB} from "../types/Price.sol";

import {console} from "forge-std/console.sol";
import {FormatLib} from "super-sol/libraries/FormatLib.sol";

struct Pair {
    address assetA;
    address assetB;
    PriceAB priceAB;
}

using PairLib for Pair global;

/// @author philogy <https://github.com/philogy>
library PairLib {
    using FormatLib for *;
    using AssetLib for *;
    using SafeCastLib for *;
    using RayMathLib for *;

    error PairAssetsWrong(Pair);

    uint256 constant MAX_U12 = (1 << 12) - 1;

    function u12(uint256 x) internal pure returns (uint256) {
        require(x <= MAX_U12, "Overflow u12");
        return x;
    }

    function _checkOrdered(Pair memory pair) internal pure {
        if (pair.assetB <= pair.assetA) revert PairAssetsWrong(pair);
    }

    function encode(Pair memory self, Asset[] memory assets) internal pure returns (bytes memory) {
        self._checkOrdered();
        (uint16 indexA, uint16 indexB) = assets.getIndexPair(self.assetA, self.assetB);
        return bytes.concat(bytes2(indexA), bytes2(indexB), bytes32(self.priceAB.into()));
    }

    function encode(Pair[] memory pairs, Asset[] memory assets) internal pure returns (bytes memory b) {
        for (uint256 i = 0; i < pairs.length; i++) {
            b = bytes.concat(b, pairs[i].encode(assets));
        }
        b = bytes.concat(bytes3(b.length.toUint24()), b);
    }

    function gt(Pair memory a, Pair memory b) internal pure returns (bool) {
        if (a.assetA == b.assetA) return a.assetB > b.assetB;
        return a.assetA > b.assetA;
    }

    function sort(Pair[] memory pairs) internal view {
        uint256 before = gasleft();

        for (uint256 i = 0; i < pairs.length; i++) {
            pairs[i]._checkOrdered();
        }

        // Bubble sort because ain't nobody got time for that.
        for (uint256 i = 0; i < pairs.length; i++) {
            for (uint256 j = i + 1; j < pairs.length; j++) {
                if (pairs[i].gt(pairs[j])) (pairs[i], pairs[j]) = (pairs[j], pairs[i]);
            }
        }

        unchecked {
            uint256 skibidi = gasleft();
            console.log("sort used: %s", before - skibidi);
        }
    }

    function getIndex(Pair[] memory pairs, address assetIn, address assetOut)
        internal
        pure
        returns (uint16 index, bool aToB)
    {
        require(assetIn != assetOut, "assetIn == assetOut");

        aToB = assetIn < assetOut;
        (address assetA, address assetB) = aToB ? (assetIn, assetOut) : (assetOut, assetIn);

        for (index = 0; index < pairs.length; index++) {
            Pair memory pair = pairs[index];
            pair._checkOrdered();
            if (pair.assetA == assetA && pair.assetB == assetB) break;
        }
        require(index < pairs.length, "Pair not found");
    }

    function toStr(Pair memory self) internal pure returns (string memory) {
        return string.concat(
            "Pair {\n    assetA: ",
            self.assetA.toStr(),
            ",\n    assetOut: ",
            self.assetB.toStr(),
            ",\n    priceAB: ",
            self.priceAB.into().fmtD(27, 27),
            "\n}"
        );
    }
}
