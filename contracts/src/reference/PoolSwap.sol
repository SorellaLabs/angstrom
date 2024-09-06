// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import {Asset, AssetLib} from "./Asset.sol";
import {PoolSwapLib as ActualPoolSwapLib} from "../types/PoolSwap.sol";
import {SafeCastLib} from "solady/src/utils/SafeCastLib.sol";

struct PoolSwap {
    address assetIn;
    address assetOut;
    uint128 amountIn;
}

using PoolSwapLib for PoolSwap global;

/// @author philogy <https://github.com/philogy>
library PoolSwapLib {
    using AssetLib for Asset[];
    using SafeCastLib for uint256;

    function encode(PoolSwap memory swap, Asset[] memory assets) internal pure returns (bytes memory b) {
        (uint16 indexA, uint16 indexB) = assets.getIndexPair({assetA: swap.assetIn, assetB: swap.assetOut});
        b = bytes.concat(bytes2(indexA), bytes2(indexB), bytes16(swap.amountIn));
        require(b.length == ActualPoolSwapLib.SWAP_BYTES);
    }

    function encode(PoolSwap[] memory swaps, Asset[] memory assets) internal pure returns (bytes memory b) {
        for (uint256 i = 0; i < swaps.length; i++) {
            b = bytes.concat(b, swaps[i].encode(assets));
        }
        b = bytes.concat(bytes3(b.length.toUint24()), b);
    }
}
