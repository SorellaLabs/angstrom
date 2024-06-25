// SPDX-License-Identifier: BUSL-1.1
pragma solidity ^0.8.0;

import {PriceGraphLib, PriceGraph, AssetIndex} from "./PriceGraph.sol";

struct Globals {
    PriceGraph prices;
    address[] assets;
}

using GlobalsLib for Globals global;

library GlobalsLib {
    function get(Globals memory self, AssetIndex index) internal pure returns (address) {
        return self.assets[index.into()];
    }
}
