// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "test/_helpers/BaseTest.sol";
import {TICK_SPACING, POOL_FEE} from "src/Constants.sol";

import {PoolGate} from "test/_helpers/PoolGate.sol";
import {TickMath} from "v4-core/src/libraries/TickMath.sol";

import {PoolId} from "v4-core/src/types/PoolId.sol";
import {PRNG} from "super-sol/collections/PRNG.sol";
import {MockERC20} from "super-sol/mocks/MockERC20.sol";
import {UniV4Inspector} from "test/_view-ext/UniV4Inspector.sol";
import {TickLib} from "src/libraries/TickLib.sol";
import {TickReward, RewardLib} from "test/_helpers/RewardLib.sol";
import {UsedIndexMap} from "super-sol/collections/UsedIndexMap.sol";
import {PoolKey} from "v4-core/src/types/PoolKey.sol";
import {PoolId, PoolIdLibrary} from "v4-core/src/types/PoolId.sol";
import {ConversionLib} from "src/libraries/ConversionLib.sol";
import {HookDeployer} from "test/_helpers/HookDeployer.sol";

import {BalanceDelta} from "v4-core/src/types/BalanceDelta.sol";

import {console} from "forge-std/console.sol";
import {FormatLib} from "super-sol/libraries/FormatLib.sol";

/// @author philogy <https://github.com/philogy>
contract UniWeirdnessTest is BaseTest {
    using FormatLib for *;

    using TickMath for int24;
    using ConversionLib for *;

    UniV4Inspector public uniV4;
    PoolGate public gate;
    PoolId public id;

    MockERC20 public asset0 = new MockERC20();
    MockERC20 public asset1 = new MockERC20();
    address public owner = makeAddr("owner");

    function setUp() public {
        if (asset1 < asset0) (asset0, asset1) = (asset1, asset0);

        uniV4 = new UniV4Inspector();
        gate = new PoolGate(address(uniV4));

        int24 startTick = 0;
        id = PoolIdLibrary.toId(address(0).toPoolKey(address(asset0), address(asset1)));
        gate.setHook(address(0));
        gate.initializePool(address(asset0), address(asset1), startTick.getSqrtPriceAtTick());

        gate.addLiquidity(address(asset0), address(asset1), -1 * TICK_SPACING, startTick, 1.0e21);
        gate.addLiquidity(address(asset0), address(asset1), -3 * TICK_SPACING, -2 * TICK_SPACING, 1.0e21);
    }

    function test_twoWaySwap() public {
        BalanceDelta delta =
            gate.swap(address(asset0), address(asset1), type(int256).min, (-2 * TICK_SPACING).getSqrtPriceAtTick());
        console.log("");
        (, int24 tick,,,,, uint128 liq) = uniV4.getPool(id);
        console.log("tick: %s", tick.toStr());
        console.log("liq: %s", liq);
        console.log("delta.amount0(): %s", delta.amount0().toStr());
        console.log("delta.amount1(): %s", delta.amount1().toStr());

        console.log("");
        delta = gate.swap(address(asset1), address(asset0), type(int256).min, int24(-60).getSqrtPriceAtTick());
        console.log("");
        console.log("delta.amount0(): %s", delta.amount0().toStr());
        console.log("delta.amount1(): %s", delta.amount1().toStr());

        (, tick,,,,,) = uniV4.getPool(id);
        console.log("tick: %s", tick.toStr());
    }

    // function swap(bool zeroForOne, int256 amount, int24 tickLimit) internal {
    //     (MockERC20 assetIn, MockERC20 assetOut) = zeroForOne ? (asset0, asset1) : (asset1, asset0);
    //     gate.swap(address(assetIn), address(assetOut), amount, tickLimit.getSqrtPriceAtTick());
    // }

    function poolKey() internal view returns (PoolKey memory) {
        return ConversionLib.toPoolKey(address(0), address(asset0), address(asset1));
    }
}
