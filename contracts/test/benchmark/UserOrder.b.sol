// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "test/_helpers/BaseTest.sol";
import {OpenAngstrom} from "test/_mocks/OpenAngstrom.sol";
import {Pair, PairLib} from "test/_reference/Pair.sol";
import {Asset, AssetLib} from "test/_reference/Asset.sol";
import {PoolConfigStore} from "src/libraries/PoolConfigStore.sol";
import {MockERC20} from "super-sol/mocks/MockERC20.sol";
import {PoolManager} from "v4-core/src/PoolManager.sol";
import {ExactFlashOrder} from "../_reference/OrderTypes.sol";
import {PriceAB} from "src/types/Price.sol";

/// @author philogy <https://github.com/philogy>
contract UserOrderBenchmarkTest is BaseTest {
    using AssetLib for *;
    using PairLib for *;

    OpenAngstrom angstrom;
    PoolManager uni;

    address asset0;
    address asset1;

    address fee_master = makeAddr("fee_master");
    address controller = makeAddr("controller");

    Account user;

    function setUp() public {
        uni = new PoolManager();
        angstrom = OpenAngstrom(
            deployAngstrom(type(OpenAngstrom).creationCode, uni, controller, fee_master)
        );
        (asset0, asset1) = deployTokensSorted();
        vm.startPrank(controller);
        angstrom.configurePool(asset0, asset1, 1, 0);
        vm.stopPrank();

        user = makeAccount("user");
    }

    function test_benchmark_exactFlashInternal() public {
        uint128 balance = 1.1e18;
        MockERC20(asset0).mint(user.addr, balance);
        uint128 other = 34_000e18;
        MockERC20(asset1).mint(user.addr, other);
        vm.startPrank(user.addr);
        MockERC20(asset0).approve(address(angstrom), type(uint256).max);
        angstrom.deposit(asset0, balance);
        MockERC20(asset1).approve(address(angstrom), type(uint256).max);
        angstrom.deposit(asset1, other);
        vm.stopPrank();

        ExactFlashOrder memory order;
        order.exactIn = true;
        order.amount = 1e18;
        order.maxExtraFeeAsset0 = 0;
        order.minPrice = 10.0e27;
        order.useInternal = true;
        order.assetIn = asset0;
        order.assetOut = asset1;
        order.validForBlock = u64(block.number);
        sign(user, order.meta, erc712Hash(computeDomainSeparator(address(angstrom)), order.hash()));

        Asset[] memory assets = new Asset[](2);
        assets[0].addr = asset0;
        assets[1].addr = asset1;
        Pair[] memory pair = new Pair[](1);
        pair[0] = Pair(asset0, asset1, PriceAB.wrap(11.5e27));

        bytes memory payload = bytes.concat(
            assets.encode(),
            pair.encode(assets, PoolConfigStore.unwrap(angstrom.configStore())),
            order.encode(pair)
        );

        vm.breakpoint("c");

        angstrom.validateAndExecuteUserOrder(payload);
    }
}

/*

    */
