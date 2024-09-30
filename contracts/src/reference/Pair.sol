// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {SafeCastLib} from "solady/src/utils/SafeCastLib.sol";
import {Asset, AssetLib} from "./Asset.sol";
import {RayMathLib} from "../libraries/RayMathLib.sol";
import {PairLib as ActualPairLib} from "../types/Pair.sol";
import {PriceAB} from "../types/Price.sol";
import {
    PoolConfigStore,
    PoolConfigStoreLib,
    StoreKey,
    STORE_HEADER_SIZE
} from "src/libraries/pool-config/PoolConfigStore.sol";
import {ConfigEntry, ENTRY_SIZE} from "src/libraries/pool-config/ConfigEntry.sol";

import {FormatLib} from "super-sol/libraries/FormatLib.sol";
import {console} from "forge-std/console.sol";

struct Pair {
    address asset0;
    address asset1;
    PriceAB price10;
}

using PairLib for Pair global;

/// @author philogy <https://github.com/philogy>
library PairLib {
    using FormatLib for *;
    using AssetLib for *;
    using SafeCastLib for *;
    using RayMathLib for *;

    error PairAssetsWrong(Pair);

    function _checkOrdered(Pair memory pair) internal pure {
        if (pair.asset1 <= pair.asset0) revert PairAssetsWrong(pair);
    }

    function encode(Pair memory self, Asset[] memory assets, address configStore)
        internal
        view
        returns (bytes memory b)
    {
        self._checkOrdered();
        (uint16 indexA, uint16 indexB) = assets.getIndexPair(self.asset0, self.asset1);
        uint16 storeIndex = getStoreIndex(configStore, self.asset0, self.asset1);
        b = bytes.concat(
            bytes2(indexA), bytes2(indexB), bytes2(storeIndex), bytes32(self.price10.into())
        );
        require(b.length == ActualPairLib.PAIR_CD_BYTES);
    }

    function encode(Pair[] memory pairs, Asset[] memory assets, address configStore)
        internal
        view
        returns (bytes memory b)
    {
        for (uint256 i = 0; i < pairs.length; i++) {
            b = bytes.concat(b, pairs[i].encode(assets, configStore));
        }
        b = bytes.concat(bytes3(b.length.toUint24()), b);
    }

    function gt(Pair memory a, Pair memory b) internal pure returns (bool) {
        if (a.asset0 == b.asset0) return a.asset1 > b.asset1;
        return a.asset0 > b.asset0;
    }

    function sort(Pair[] memory pairs) internal pure {
        for (uint256 i = 0; i < pairs.length; i++) {
            pairs[i]._checkOrdered();
        }

        // Bubble sort because ain't nobody got time for that.
        for (uint256 i = 0; i < pairs.length; i++) {
            for (uint256 j = i + 1; j < pairs.length; j++) {
                if (pairs[i].gt(pairs[j])) (pairs[i], pairs[j]) = (pairs[j], pairs[i]);
            }
        }
    }

    function getIndex(Pair[] memory pairs, address assetIn, address assetOut)
        internal
        pure
        returns (uint16 index, bool zeroForOne)
    {
        require(assetIn != assetOut, "assetIn == assetOut");

        zeroForOne = assetIn < assetOut;
        (address asset0, address asset1) = zeroForOne ? (assetIn, assetOut) : (assetOut, assetIn);

        for (index = 0; index < pairs.length; index++) {
            Pair memory pair = pairs[index];
            pair._checkOrdered();
            if (pair.asset0 == asset0 && pair.asset1 == asset1) return (index, zeroForOne);
        }
        require(index < pairs.length, "Pair not found");
    }

    function getStoreIndex(address store, address asset0, address asset1)
        internal
        view
        returns (uint16 index)
    {
        require(asset0 < asset1, "getStoreIndex:assets unsorted");
        StoreKey key = PoolConfigStoreLib.keyFromAssetsUnchecked(asset0, asset1);
        uint256 totalEntries = store.code.length / ENTRY_SIZE;
        PoolConfigStore configStore = PoolConfigStore.wrap(store);
        for (index = 0; index < totalEntries; index++) {
            StoreKey entryKey;
            assembly ("memory-safe") {
                extcodecopy(
                    configStore, 0x00, add(STORE_HEADER_SIZE, mul(ENTRY_SIZE, index)), ENTRY_SIZE
                )
                entryKey := mload(0x00)
            }
            if (StoreKey.unwrap(entryKey) == StoreKey.unwrap(key)) return index;
        }
        revert("Pool not enabled");
    }

    function toStr(Pair memory self) internal pure returns (string memory) {
        return string.concat(
            "Pair {\n    asset0: ",
            self.asset0.toStr(),
            ",\n    asset1: ",
            self.asset1.toStr(),
            ",\n    price10: ",
            self.price10.into().fmtD(27, 27),
            "\n}"
        );
    }
}
