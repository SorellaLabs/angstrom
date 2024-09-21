// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import {UserOrder, UserOrderLib} from "./UserOrder.sol";
import {Asset, AssetLib} from "./Asset.sol";
import {Pair, PairLib} from "./Pair.sol";
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

    function encode(Bundle memory self) internal pure returns (bytes memory) {
        return bytes.concat(
            self.assets.encode(),
            self.pairs.encode(self.assets),
            self.poolUpdates.encode(self.assets),
            self.toBOrders.encode(self.assets),
            self.userOrders.encode(self.pairs)
        );
    }

    function addDeltas(Bundle memory self, uint256 index0, uint256 index1, BalanceDelta deltas) internal pure {
        self.assets[index0].addDelta(deltas.amount0());
        self.assets[index1].addDelta(deltas.amount1());
    }
}
