// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import {UserOrder, UserOrderLib} from "./UserOrder.sol";
import {Asset, AssetLib} from "./Asset.sol";
import {Pair, PairLib} from "./Pair.sol";
import {OrdersLib, TopOfBlockOrder} from "./OrderTypes.sol";
import {PoolSwap, PoolSwapLib} from "./PoolSwap.sol";
import {PoolRewardsUpdate, PoolRewardsUpdateLib} from "./PoolRewardsUpdate.sol";

import {console} from "forge-std/console.sol";

struct Bundle {
    Asset[] assets;
    Pair[] pairs;
    PoolSwap[] swaps;
    TopOfBlockOrder[] toBOrders;
    UserOrder[] userOrders;
    PoolRewardsUpdate[] poolRewardsUpdates;
}

using BundleLib for Bundle global;

/// @author philogy <https://github.com/philogy>
library BundleLib {
    using OrdersLib for TopOfBlockOrder[];
    using UserOrderLib for UserOrder[];
    using AssetLib for Asset[];
    using PairLib for Pair[];
    using PoolSwapLib for PoolSwap[];
    using PoolRewardsUpdateLib for PoolRewardsUpdate[];

    function encode(Bundle memory bundle) internal pure returns (bytes memory) {
        return bytes.concat(
            bundle.assets.encode(),
            bundle.pairs.encode(bundle.assets),
            bundle.swaps.encode(bundle.assets),
            bundle.toBOrders.encode(bundle.assets),
            bundle.userOrders.encode(bundle.pairs),
            bundle.poolRewardsUpdates.encode(bundle.assets)
        );
    }
}
