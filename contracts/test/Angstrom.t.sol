// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "test/_helpers/BaseTest.sol";
import {PoolManager} from "v4-core/src/PoolManager.sol";
import {Angstrom} from "src/Angstrom.sol";
import {Bundle} from "test/_reference/Bundle.sol";
import {Asset, AssetLib} from "test/_reference/Asset.sol";
import {Pair, PairLib} from "test/_reference/Pair.sol";
import {UserOrder, UserOrderLib} from "test/_reference/UserOrder.sol";
import {PartialStandingOrder, ExactFlashOrder} from "test/_reference/OrderTypes.sol";
import {PriceAB as Price10} from "src/types/Price.sol";
import {MockERC20} from "super-sol/mocks/MockERC20.sol";

import {console} from "forge-std/console.sol";

/// @author philogy <https://github.com/philogy>
contract AngstromTest is BaseTest {
    using PairLib for Pair[];
    using AssetLib for Asset[];

    PoolManager uni;
    Angstrom angstrom;
    bytes32 domainSeparator;

    address controller = makeAddr("controller");
    address node = makeAddr("node");
    address asset0;
    address asset1;

    function setUp() public {
        uni = new PoolManager(address(0));
        angstrom = Angstrom(deployAngstrom(type(Angstrom).creationCode, uni, controller));
        domainSeparator = computeDomainSeparator(address(angstrom));

        vm.prank(controller);
        angstrom.toggleNodes(addressArray(abi.encode(node)));

        (asset0, asset1) = deployTokensSorted();

        MockERC20(asset0).mint(address(uni), 100_000e18);
        MockERC20(asset1).mint(address(uni), 100_000e18);
    }

    function test_userOrderWithFees() public {
        uint256 fee = 0.002e6;

        vm.prank(controller);
        angstrom.configurePool(asset0, asset1, 1, uint24(fee));

        console.log("asset0: %s", asset0);
        console.log("asset1: %s", asset1);

        Account memory user1 = makeAccount("user_1");
        MockERC20(asset0).mint(user1.addr, 100.0e18);
        vm.prank(user1.addr);
        MockERC20(asset0).approve(address(angstrom), type(uint256).max);

        Account memory user2 = makeAccount("user_2");
        MockERC20(asset1).mint(user2.addr, 100.0e18);
        vm.prank(user2.addr);
        MockERC20(asset1).approve(address(angstrom), type(uint256).max);

        Price10 price = Price10.wrap(1e27);

        Bundle memory bundle;

        bundle.addAsset(asset0).addAsset(asset1).addPair(asset0, asset1, price);
        bundle.userOrders = new UserOrder[](2);

        {
            PartialStandingOrder memory order;
            order.maxAmountIn = 20.0e18;
            order.maxExtraFeeAsset0 = 1.3e18;
            order.minPrice = 0.1e27;
            order.assetIn = asset0;
            order.assetOut = asset1;
            order.deadline = u40(block.timestamp + 60 minutes);
            sign(user1, order.meta, digest712(order.hash()));
            order.extraFeeAsset0 = 1.0e18;
            order.amountFilled = 10.0e18;
            bundle.userOrders[0] = UserOrderLib.from(order);
        }

        {
            ExactFlashOrder memory order;
            order.exactIn = true;
            order.amount = 9.200400801603206413e18;
            order.maxExtraFeeAsset0 = 0.2e18;
            order.minPrice = 0.1e27;
            order.assetIn = asset1;
            order.assetOut = asset0;
            order.validForBlock = u64(block.number);
            sign(user2, order.meta, digest712(order.hash()));
            order.extraFeeAsset0 = 0.2e18;
            bundle.userOrders[1] = UserOrderLib.from(order);
        }

        bundle.assets[0].save += 1.018e18;
        bundle.assets[1].save += 0.218400801603206413e18;
        bundle.assets[1].take += 10.0e18;
        bundle.assets[1].settle += 10.0e18;

        bytes memory payload = bundle.encode(rawGetConfigStore(address(angstrom)));
        vm.prank(node);
        angstrom.execute(payload);
    }

    function digest712(bytes32 structHash) internal view returns (bytes32) {
        return erc712Hash(domainSeparator, structHash);
    }
}
