// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import {UserOrder, UserOrderLib} from "./UserOrder.sol";
import {Asset, AssetLib} from "./Asset.sol";
import {Pair, PairLib} from "./Pair.sol";
import {PriceAB as Price10} from "../types/Price.sol";
import {TopOfBlockOrder, OrdersLib} from "./OrderTypes.sol";
import {PoolUpdate, PoolUpdateLib} from "./PoolUpdate.sol";
import {BalanceDelta} from "v4-core/src/types/BalanceDelta.sol";

import {console} from "forge-std/console.sol";

struct Bundle {
    Asset[] assets;
    Pair[] pairs;
    PoolUpdate[] poolUpdates;
    TopOfBlockOrder[] toBOrders;
    UserOrder[] userOrders;
}

using BundleLib for Bundle global;

/// @author philogy <https://github.com/philogy>
library BundleLib {
    using OrdersLib for TopOfBlockOrder[];
    using UserOrderLib for UserOrder[];
    using AssetLib for Asset[];
    using PairLib for Pair[];
    using PoolUpdateLib for PoolUpdate[];

    function encode(Bundle memory self, address configStore) internal view returns (bytes memory) {
        return bytes.concat(
            self.assets.encode(),
            self.pairs.encode(self.assets, configStore),
            self.poolUpdates.encode(self.pairs),
            self.toBOrders.encode(self.pairs),
            self.userOrders.encode(self.pairs)
        );
    }

    function feeSummary(Bundle memory self) internal pure returns (bytes32) {
        bytes memory summaryPreImage;
        for (uint256 i = 0; i < self.assets.length; i++) {
            Asset memory asset = self.assets[i];
            summaryPreImage =
                bytes.concat(summaryPreImage, bytes20(asset.addr), bytes16(asset.save));
        }
        return keccak256(summaryPreImage);
    }

    function getAsset(Bundle memory self, address addr) internal pure returns (Asset memory) {
        for (uint256 i = 0; i < self.assets.length; i++) {
            Asset memory asset = self.assets[i];
            if (asset.addr == addr) return asset;
        }
        revert("asset not found");
    }

    function addAsset(Bundle memory self, address newAddr) internal pure {
        for (uint256 i = 0; i < self.assets.length; i++) {
            Asset memory asset = self.assets[i];
            if (asset.addr == newAddr) return;
        }
        Asset[] memory newAssets = new Asset[](self.assets.length + 1);
        for (uint256 i = 0; i < self.assets.length; i++) {
            newAssets[i] = self.assets[i];
        }
        newAssets[self.assets.length].addr = newAddr;
        self.assets = newAssets;
    }

    function addPair(Bundle memory self, address asset0, address asset1)
        internal
        pure
        returns (Bundle memory)
    {
        return self.addPair(asset0, asset1, Price10.wrap(0));
    }

    function addPair(Bundle memory self, address asset0, address asset1, Price10 price10)
        internal
        pure
        returns (Bundle memory)
    {
        if (asset0 > asset1) (asset0, asset1) = (asset1, asset0);
        self.addAsset(asset0);
        self.addAsset(asset1);
        for (uint256 i = 0; i < self.pairs.length; i++) {
            Pair memory pair = self.pairs[i];
            if (pair.asset0 == asset0 && pair.asset1 == asset1) return self;
        }
        Pair[] memory newPairs = new Pair[](self.pairs.length + 1);
        for (uint256 i = 0; i < self.pairs.length; i++) {
            newPairs[i] = self.pairs[i];
        }
        newPairs[self.pairs.length].asset0 = asset0;
        newPairs[self.pairs.length].asset1 = asset1;
        newPairs[self.pairs.length].price10 = price10;
        self.pairs = newPairs;
        return self;
    }

    function addToB(Bundle memory self, TopOfBlockOrder memory tob)
        internal
        pure
        returns (Bundle memory)
    {
        self.addPair(tob.assetIn, tob.assetOut);

        TopOfBlockOrder[] memory newToBOrders = new TopOfBlockOrder[](self.toBOrders.length + 1);
        for (uint256 i = 0; i < self.toBOrders.length; i++) {
            newToBOrders[i] = self.toBOrders[i];
        }
        newToBOrders[self.toBOrders.length] = tob;
        self.toBOrders = newToBOrders;

        return self;
    }

    function addDeltas(Bundle memory self, uint256 index0, uint256 index1, BalanceDelta deltas)
        internal
        pure
    {
        self.assets[index0].addDelta(deltas.amount0());
        self.assets[index1].addDelta(deltas.amount1());
    }
}
