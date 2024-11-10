// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "test/_helpers/BaseTest.sol";
import {Angstrom} from "src/Angstrom.sol";
import {Pair, PairLib} from "test/_reference/Pair.sol";
import {Asset, AssetLib} from "test/_reference/Asset.sol";
import {Bundle} from "test/_reference/Bundle.sol";
import {PoolConfigStore} from "src/libraries/PoolConfigStore.sol";
import {MockERC20} from "super-sol/mocks/MockERC20.sol";
import {PoolManager} from "v4-core/src/PoolManager.sol";
import {ExactFlashOrder, ExactStandingOrder, TopOfBlockOrder} from "../_reference/OrderTypes.sol";
import {UserOrder, UserOrderLib} from "../_reference/UserOrder.sol";
import {PriceAB} from "src/types/Price.sol";

import {console} from "forge-std/console.sol";

/// @author philogy <https://github.com/philogy>
contract FullBundleBenchmark is BaseTest {
    using AssetLib for *;
    using PairLib for *;

    Angstrom angstrom;
    PoolManager uni;

    address asset0;
    address asset1;

    address fee_master = makeAddr("fee_master");
    address controller = makeAddr("controller");
    address node = makeAddr("the_one");

    function setUp() public {
        uni = new PoolManager(address(0));
        angstrom = Angstrom(deployAngstrom(type(Angstrom).creationCode, uni, controller));
        (asset0, asset1) = deployTokensSorted();
        vm.startPrank(controller);
        angstrom.configurePool(asset0, asset1, 1, 0);
        angstrom.toggleNodes(addressArray(abi.encode(node)));
        vm.stopPrank();
    }

    function test_exactFlashInternal_1() public {
        _bundleWithExactFlashInternal(1);
    }

    function test_exactFlashInternal_2() public {
        _bundleWithExactFlashInternal(2);
    }

    function test_exactStandingLiquidNonZeroNonce_1() public {
        _bundleWithExactStandingLiquid(1);
    }

    function test_exactStandingLiquidNonZeroNonce_2() public {
        _bundleWithExactStandingLiquid(2);
    }

    function test_exactStandingLiquidNonZeroNonce_3() public {
        _bundleWithExactStandingLiquid(3);
    }

    function _bundleWithExactFlashInternal(uint256 total) internal {
        Bundle memory bundle;
        uint128 amountIn = 1.0e18;
        uint128 amountOut = 1.0e18;
        uint128 fee0 = 0.00038e18;

        bundle.userOrders = new UserOrder[](total);

        for (uint256 i = 0; i < total; i++) {
            Account memory user = makeAccount(string.concat("user_", vm.toString(i + 1)));

            {
                uint128 amount0 = amountIn + fee0 + 2.0e18;
                vm.startPrank(user.addr);
                MockERC20(asset0).mint(user.addr, amount0);
                MockERC20(asset0).approve(address(angstrom), type(uint256).max);
                angstrom.deposit(asset0, amount0);
                vm.stopPrank();
            }
            {
                uint128 amount1 = amountOut + 2.0e18;
                vm.startPrank(user.addr);
                MockERC20(asset1).mint(user.addr, amount1);
                MockERC20(asset1).approve(address(angstrom), type(uint256).max);
                angstrom.deposit(asset1, amount1);
                vm.stopPrank();
            }

            ExactFlashOrder memory order;
            order.exactIn = true;
            order.amount = amountIn + fee0;
            order.maxExtraFeeAsset0 = 0;
            order.minPrice = 0.9e27;
            order.useInternal = true;
            order.assetIn = asset0;
            order.assetOut = asset1;
            order.maxExtraFeeAsset0 = fee0;
            order.extraFeeAsset0 = fee0;
            order.validForBlock = u64(block.number);
            sign(
                user,
                order.meta,
                erc712Hash(computeDomainSeparator(address(angstrom)), order.hash())
            );

            bundle.userOrders[i] = UserOrderLib.from(order);
        }

        bundle.assets = new Asset[](2);
        bundle.assets[0] = Asset(asset0, uint128(total) * fee0, 0, 0);
        bundle.assets[1] = Asset(asset1, 0, 0, 0);
        bundle.pairs = new Pair[](1);
        bundle.pairs[0] = Pair(asset0, asset1, PriceAB.wrap(1.0e27));

        {
            Account memory mr_tob = makeAccount("mr_tob");

            uint128 tobIn = amountOut * uint128(total);
            uint128 tobFee0 = 0.05e18;
            uint128 tobOut = amountIn * uint128(total) + tobFee0;

            {
                uint128 amount1 = tobIn + 100.0e18;
                uint128 amount0 = tobOut + 100.0e18;
                vm.startPrank(mr_tob.addr);

                MockERC20(asset0).mint(mr_tob.addr, amount0);
                MockERC20(asset0).approve(address(angstrom), type(uint256).max);
                angstrom.deposit(asset0, amount0);

                MockERC20(asset1).mint(mr_tob.addr, amount1);
                MockERC20(asset1).approve(address(angstrom), type(uint256).max);
                angstrom.deposit(asset1, amount1);

                vm.stopPrank();
            }

            TopOfBlockOrder memory tob;
            tob.quantityIn = tobIn;
            tob.quantityOut = tobOut;
            tob.maxGasAsset0 = 0.1e18;
            tob.useInternal = true;
            tob.assetIn = asset1;
            tob.assetOut = asset0;
            tob.validForBlock = uint64(block.number);
            tob.gasUsedAsset0 = tobFee0;

            sign(
                mr_tob, tob.meta, erc712Hash(computeDomainSeparator(address(angstrom)), tob.hash())
            );

            bundle.addToB(tob);

            address config = rawGetConfigStore(address(angstrom));
            bytes memory payload = bundle.encode(config);
            uint256 cdCost = 0;
            for (uint256 i = 0; i < payload.length; i++) {
                if (payload[i] == 0x00) {
                    cdCost += 4;
                } else {
                    cdCost += 16;
                }
            }
            console.log("cdCost: %s", cdCost);

            vm.breakpoint("c");
            vm.prank(node);

            angstrom.execute(payload);
        }
    }

    function _bundleWithExactStandingLiquid(uint256 total) internal {
        Bundle memory bundle;
        uint128 amountIn = 1.0e18;
        uint128 amountOut = 1.0e18;
        uint128 fee0 = 0.00038e18;

        bundle.userOrders = new UserOrder[](total);

        MockERC20(asset0).mint(address(angstrom), 1000.0e18);
        MockERC20(asset1).mint(address(angstrom), 1000.0e18);

        for (uint256 i = 0; i < total; i++) {
            Account memory user = makeAccount(string.concat("user_", vm.toString(i + 1)));

            {
                uint128 amount0 = amountIn + fee0 + 2.0e18;
                uint128 amount1 = amountOut + 2.0e18;
                vm.startPrank(user.addr);
                MockERC20(asset0).mint(user.addr, amount0);
                MockERC20(asset0).approve(address(angstrom), type(uint256).max);
                angstrom.invalidateNonce(0);
                MockERC20(asset1).mint(user.addr, amount1);
                MockERC20(asset1).approve(address(angstrom), type(uint256).max);
                vm.stopPrank();
            }

            ExactStandingOrder memory order;
            order.exactIn = true;
            order.amount = amountIn + fee0;
            order.maxExtraFeeAsset0 = 0;
            order.minPrice = 0.9e27;
            order.useInternal = false;
            order.assetIn = asset0;
            order.assetOut = asset1;
            order.maxExtraFeeAsset0 = fee0;
            order.extraFeeAsset0 = fee0;
            order.nonce = 1;
            order.deadline = uint40(vm.unixTime()) / 1000 + 60 minutes;
            sign(
                user,
                order.meta,
                erc712Hash(computeDomainSeparator(address(angstrom)), order.hash())
            );

            bundle.userOrders[i] = UserOrderLib.from(order);
        }

        bundle.assets = new Asset[](2);
        bundle.assets[0] = Asset(asset0, uint128(total) * fee0, 0, 0);
        bundle.assets[1] = Asset(asset1, 0, 0, 0);
        bundle.pairs = new Pair[](1);
        bundle.pairs[0] = Pair(asset0, asset1, PriceAB.wrap(1.0e27));

        {
            Account memory mr_tob = makeAccount("mr_tob");

            uint128 tobIn = amountOut * uint128(total);
            uint128 tobFee0 = 0.05e18;
            uint128 tobOut = amountIn * uint128(total) + tobFee0;

            {
                console.log("mr_tob.addr: %s", mr_tob.addr);
                uint128 amount1 = tobIn + 100.0e18;
                uint128 amount0 = tobOut + 100.0e18;
                vm.startPrank(mr_tob.addr);

                MockERC20(asset0).mint(mr_tob.addr, amount0);
                MockERC20(asset0).approve(address(angstrom), type(uint256).max);
                angstrom.deposit(asset0, amount0);

                MockERC20(asset1).mint(mr_tob.addr, amount1);
                MockERC20(asset1).approve(address(angstrom), type(uint256).max);
                angstrom.deposit(asset1, amount1);

                vm.stopPrank();
            }

            TopOfBlockOrder memory tob;
            tob.quantityIn = tobIn;
            tob.quantityOut = tobOut;
            tob.maxGasAsset0 = 0.1e18;
            tob.useInternal = true;
            tob.assetIn = asset1;
            tob.assetOut = asset0;
            tob.validForBlock = uint64(block.number);
            tob.gasUsedAsset0 = tobFee0;

            sign(
                mr_tob, tob.meta, erc712Hash(computeDomainSeparator(address(angstrom)), tob.hash())
            );

            bundle.addToB(tob);

            address config = rawGetConfigStore(address(angstrom));
            console.log("config: %s", config);
            bytes memory payload = bundle.encode(config);
            uint256 cdCost = 0;
            for (uint256 i = 0; i < payload.length; i++) {
                if (payload[i] == 0x00) {
                    cdCost += 4;
                } else {
                    cdCost += 16;
                }
            }
            console.log("cdCost: %s", cdCost);

            vm.breakpoint("c");
            vm.prank(node);

            angstrom.execute(payload);
        }
    }
}
