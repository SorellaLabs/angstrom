// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import {console2 as console} from "forge-std/console2.sol";

struct PairItem {
    address asset0;
    address asset1;
    uint256 price;
    uint256 orderCount;
    uint256 orderOffset;
}

struct PairIterator {
    address[] assets;
    uint256[] prices;
    uint256[] orderCounts;
    uint256 asset0Index;
    uint256 asset1Index;
    uint256 pi;
}

using PairIteratorLib for PairIterator global;

/// @author philogy <https://github.com/philogy>
library PairIteratorLib {
    function init(address[] memory assets, uint256[] memory prices, uint256[] memory orderCounts)
        internal
        pure
        returns (PairIterator memory iter)
    {
        require(assets.length >= 2, "Too few assets");
        require(prices.length == orderCounts.length, "Order count prices length mismatch");
        require(pyr(assets.length) == prices.length, "Invalid pairs for assets");
        {
            address lastAsset = address(0);
            for (uint256 i = 0; i < assets.length; i++) {
                address asset = assets[i];
                require(asset > lastAsset, "Assets out-of-order");
                lastAsset = asset;
            }
        }
        iter.assets = assets;
        iter.prices = prices;
        iter.orderCounts = orderCounts;
        iter.reset();
    }

    function reset(PairIterator memory self) internal pure {
        self.asset0Index = 0;
        self.asset1Index = 1;
        self.pi = 0;
    }

    function totalPairs(PairIterator memory self) internal pure returns (uint256) {
        return self.prices.length;
    }

    function totalOrders(PairIterator memory self) internal pure returns (uint256 total) {
        for (uint256 i = 0; i < self.orderCounts.length; i++) {
            total += self.orderCounts[i];
        }
    }

    function next(PairIterator memory self) internal pure returns (PairItem memory pair) {
        require(!self.done(), "Iterator done");
        pair.asset0 = self.assets[self.asset0Index];
        pair.asset1 = self.assets[self.asset1Index];
        pair.price = self.prices[self.pi];
        pair.orderCount = self.orderCounts[self.pi];
        for (uint256 i = 0; i < self.pi; i++) {
            pair.orderOffset += self.orderCounts[i];
        }
        // Progress pointers.
        self.pi++;
        self.asset1Index++;
        if (self.asset1Index == self.assets.length) {
            self.asset0Index++;
            self.asset1Index = self.asset0Index + 1;
        }
    }

    function done(PairIterator memory self) internal pure returns (bool) {
        return self.asset0Index + 1 == self.asset1Index && self.asset1Index >= self.assets.length;
    }

    /**
     * @dev Returns the n-th pyramid number.
     */
    function pyr(uint256 n) internal pure returns (uint256 x) {
        unchecked {
            x = n * (n - 1) / 2;
        }
    }
}
