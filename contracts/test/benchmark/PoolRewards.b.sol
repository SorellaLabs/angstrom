// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "test/_helpers/BaseTest.sol";
import {Trader} from "test/_helpers/types/Trader.sol";
import {PoolManager} from "v4-core/src/PoolManager.sol";
import {IUniV4, IPoolManager} from "src/interfaces/IUniV4.sol";
import {PoolGate} from "test/_helpers/PoolGate.sol";
import {Angstrom} from "src/Angstrom.sol";
import {MockERC20} from "super-sol/mocks/MockERC20.sol";
import {PoolId} from "v4-core/src/types/PoolId.sol";
import {DeltaClearerERC20} from "test/_helpers/DeltaClearerERC20.sol";
import {RewardLib, TickReward} from "../_helpers/RewardLib.sol";

import {PoolKey} from "v4-core/src/types/PoolKey.sol";
import {TickMath} from "v4-core/src/libraries/TickMath.sol";
import {console} from "forge-std/console.sol";

import {Bundle, Asset, Pair, PoolUpdate} from "test/_reference/Bundle.sol";
import {PairLib} from "test/_reference/Pair.sol";
import {PriceAB} from "src/types/Price.sol";
import {TopOfBlockOrder} from "test/_reference/OrderTypes.sol";
import {PoolUpdate, RewardsUpdate} from "test/_reference/PoolUpdate.sol";

import {console} from "forge-std/console.sol";
import {FormatLib} from "super-sol/libraries/FormatLib.sol";

/// @author philogy <https://github.com/philogy>
contract PoolRewardsTest is BaseTest {
    using FormatLib for *;
    using TickMath for int24;
    using IUniV4 for PoolManager;

    PoolGate gate;
    PoolManager uni;
    Angstrom angstrom;
    DeltaClearerERC20 clearer;
    address controller = makeAddr("controller");

    Trader searcher;

    PoolId id;
    address asset0;
    address asset1;
    int24 tickSpacing;

    address configStore;

    bytes32 domainSeparator;

    function setUp() public {
        searcher = makeTrader("searcher");

        uni = new PoolManager();
        gate = new PoolGate(address(uni));
        angstrom = Angstrom(deployAngstrom(type(Angstrom).creationCode, uni, controller));
        gate.setHook(address(angstrom));

        (asset0, asset1) = deployTokensSorted();
        clearer = new DeltaClearerERC20(address(angstrom), uni);
        clearer.addClear(asset0);
        clearer.addClear(asset1);

        address[] memory nodes = new address[](1);
        nodes[0] = controller;
        vm.prank(controller);
        angstrom.toggleNodes(nodes);

        vm.startPrank(controller);
        angstrom.configurePool(asset0, asset1, 60, 0);
        angstrom.configurePool(asset0, address(clearer), 1, 0);
        vm.stopPrank();
        // Note hardcoded slot for `Angstrom.sol`, might be different for test derivations.
        configStore = rawGetConfigStore(address(angstrom));
        domainSeparator = computeDomainSeparator(address(angstrom));

        gate.tickSpacing(tickSpacing = 60);
        id = gate.initializePool(
            asset0,
            asset1,
            int24(4).getSqrtPriceAtTick(),
            PairLib.getStoreIndex(configStore, asset0, asset1)
        );

        gate.addLiquidity(asset0, asset1, -60, 0, 1e21, bytes32(0));
        gate.addLiquidity(asset0, asset1, 0, 60, 1e21, bytes32(0));
        gate.addLiquidity(asset0, asset1, 60, 120, 1e21, bytes32(0));

        vm.startPrank(searcher.addr);
        MockERC20(asset0).approve(address(angstrom), type(uint256).max);
        MockERC20(asset0).mint(searcher.addr, 1e26);
        MockERC20(asset1).approve(address(angstrom), type(uint256).max);
        MockERC20(asset1).mint(searcher.addr, 1e26);
        vm.stopPrank();

        updatePoolZeroToOne(0.4e18, RewardLib.CurrentOnly(4.0e18));
        console.log("tick: %s", uni.getSlot0(id).tick().toStr());
        bumpBlock();

        updatePoolOneToZero(3.4e18, RewardLib.CurrentOnly(4.0e18));
        console.log("tick: %s", uni.getSlot0(id).tick().toStr());
        bumpBlock();

        updatePoolZeroToOne(1.9e18, RewardLib.CurrentOnly(4.0e18));
        console.log("tick: %s", uni.getSlot0(id).tick().toStr());
        bumpBlock();
    }

    function test_benchmark_emptyBundle() public {
        Bundle memory bundle;

        bytes memory encodedPayload = bundle.encode(address(0));
        vm.prank(controller);
        angstrom.execute(encodedPayload);
    }

    modifier reportTickChange() {
        console.log("tick before: %s", uni.getSlot0(id).tick().toStr());
        _;
        console.log("tick after: %s", uni.getSlot0(id).tick().toStr());
    }

    function test_bench_rewardCurrent_swapWithin() public {
        updatePoolZeroToOne(1e14, RewardLib.CurrentOnly(3.2e18));
    }

    function test_bench_rewardCurrent_noSwap() public {
        updatePoolZeroToOne(0, RewardLib.CurrentOnly(3.2e18));
    }

    function test_bench_rewardCurrent_crossTick() public reportTickChange {
        updatePoolZeroToOne(1.4e18, RewardLib.CurrentOnly(3.2e18));
    }

    function test_bench_rewardMultiOneWord_swapWithin() public reportTickChange {
        RewardsUpdate[] memory updates = RewardLib.toUpdates(
            RewardLib.re(TickReward(60, 1.0e18), TickReward(0, 1.0e18)), uni, id, tickSpacing
        );
        assertEq(updates.length, 1);
        updatePoolOneToZero(3.33e14, updates[0]);
    }

    function test_bench_rewardMultiMultiWord_swapAcross() public {
        RewardsUpdate[] memory updates = RewardLib.toUpdates(
            RewardLib.re(TickReward(-60, 1.0e18), TickReward(0, 1.0e18)), uni, id, tickSpacing, -60
        );
        assertEq(updates.length, 1);
        updatePoolZeroToOne(1.7e18, updates[0]);
    }

    function updatePoolZeroToOne(uint128 swapIn, RewardsUpdate memory rewards) internal {
        Bundle memory bundle;

        bundle.addAsset(asset0).addAsset(asset1).addAsset(address(clearer));
        bundle.getAsset(asset0).settle = swapIn;

        // forgefmt: disable-next-item
        bundle
            .addPair(asset0, asset1, PriceAB.wrap(0x6c6b935b8bbd4111111))
            .addPair(asset0, address(clearer), PriceAB.wrap(0));

        bundle.poolUpdates = new PoolUpdate[](1);
        PoolUpdate memory update = bundle.poolUpdates[0];
        update.assetIn = asset0;
        update.assetOut = asset1;
        update.amountIn = swapIn;
        update.rewardUpdate = rewards;

        bundle.toBOrders = new TopOfBlockOrder[](1);
        TopOfBlockOrder memory tob = bundle.toBOrders[0];
        tob.quantityIn = rewards.total() + swapIn;
        tob.assetIn = asset0;
        tob.assetOut = address(clearer);
        tob.validForBlock = u64(block.number);
        sign(searcher, tob.meta, erc712Hash(domainSeparator, tob.hash()));

        bytes memory encodedPayload = bundle.encode(configStore);
        vm.prank(controller);
        angstrom.execute(encodedPayload);
    }

    function updatePoolOneToZero(uint128 swapIn, RewardsUpdate memory rewards) internal {
        Bundle memory bundle;

        bundle.addAsset(asset0).addAsset(asset1).addAsset(address(clearer));
        bundle.getAsset(asset1).settle = swapIn;

        bundle.addPair(asset0, asset1, PriceAB.wrap(0x6c6b935b8bbd4111111));
        bundle.addPair(asset0, address(clearer), PriceAB.wrap(0));

        bundle.poolUpdates = new PoolUpdate[](1);
        PoolUpdate memory update = bundle.poolUpdates[0];
        update.assetIn = asset1;
        update.assetOut = asset0;
        update.amountIn = swapIn;
        update.rewardUpdate = rewards;

        bundle.toBOrders = new TopOfBlockOrder[](2);
        TopOfBlockOrder memory tob = bundle.toBOrders[0];
        tob.quantityIn = rewards.total();
        tob.assetIn = asset0;
        tob.assetOut = address(clearer);
        tob.validForBlock = u64(block.number);
        sign(searcher, tob.meta, erc712Hash(domainSeparator, tob.hash()));

        tob = bundle.toBOrders[1];
        tob.quantityIn = swapIn;
        tob.assetIn = asset1;
        tob.assetOut = asset0;
        tob.validForBlock = u64(block.number);
        sign(searcher, tob.meta, erc712Hash(domainSeparator, tob.hash()));

        bytes memory encodedPayload = bundle.encode(configStore);
        vm.breakpoint("c");
        vm.prank(controller);
        angstrom.execute(encodedPayload);
    }
}
