// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "test/_helpers/BaseTest.sol";
import {PoolManager} from "v4-core/src/PoolManager.sol";
import {TickMath} from "v4-core/src/libraries/TickMath.sol";
import {Angstrom} from "src/Angstrom.sol";
import {TopLevelAuth} from "src/modules/TopLevelAuth.sol";
import {Bundle} from "test/_reference/Bundle.sol";
import {Asset, AssetLib} from "test/_reference/Asset.sol";
import {Pair, PairLib} from "test/_reference/Pair.sol";
import {UserOrder, UserOrderLib} from "test/_reference/UserOrder.sol";
import {PartialStandingOrder, ExactFlashOrder} from "test/_reference/OrderTypes.sol";
import {PriceAB as Price10} from "src/types/Price.sol";
import {MockERC20} from "super-sol/mocks/MockERC20.sol";
import {AngstromView} from "src/periphery/AngstromView.sol";
import {RouterActor, PoolKey} from "test/_mocks/RouterActor.sol";

import {IHooks} from "v4-core/src/interfaces/IHooks.sol";
import {Hooks} from "v4-core/src/libraries/Hooks.sol";

// to force compile
import {IPositionDescriptor} from "v4-periphery/src/interfaces/IPositionDescriptor.sol";
import {console} from "forge-std/console.sol";

/// @author philogy <https://github.com/philogy>
contract AngstromTest is BaseTest {
    using AngstromView for Angstrom;

    using Hooks for IHooks;

    using PairLib for Pair[];
    using AssetLib for Asset[];

    PoolManager uni;
    Angstrom angstrom;
    bytes32 domainSeparator;

    address controller = makeAddr("controller");
    Account node = makeAccount("node");
    address asset0;
    address asset1;

    RouterActor actor;

    function setUp() public {
        uni = new PoolManager(address(0));
        angstrom = Angstrom(deployAngstrom(type(Angstrom).creationCode, uni, controller));
        domainSeparator = computeDomainSeparator(address(angstrom));

        vm.prank(controller);
        angstrom.toggleNodes(addressArray(abi.encode(node.addr)));

        actor = new RouterActor(uni);

        (asset0, asset1) = deployTokensSorted();
        MockERC20(asset0).mint(address(uni), 100_000e18);
        MockERC20(asset1).mint(address(uni), 100_000e18);

        MockERC20(asset0).mint(address(actor), 100_000_000e18);
        MockERC20(asset1).mint(address(actor), 100_000_000e18);

        assertEq(angstrom.lastBlockUpdated(), 0);
    }

    function test_userOrderWithFees() public {
        uint256 fee = 0.002e6;

        vm.prank(controller);
        angstrom.configurePool(asset0, asset1, 1, uint24(fee), 0);

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
        vm.prank(node.addr);
        angstrom.execute(payload);
    }

    function test_fuzzing_shortCircuitEmptyBundle(uint256 bn1, uint256 bn2) public {
        uint64 block1 = bound_block(bn1, type(uint40).max);
        uint64 block2 = uint64(bound(bn2, uint64(block1) + 1, type(uint64).max));

        Bundle memory bundle;
        bytes memory payload = bundle.encode(angstrom.configStore().into());

        vm.roll(block1);
        vm.prank(node.addr);
        angstrom.execute(payload);

        assertEq(angstrom.lastBlockUpdated(), block1);

        vm.prank(node.addr);
        vm.expectRevert(TopLevelAuth.OnlyOncePerBlock.selector);
        angstrom.execute(payload);

        vm.roll(block2);
        vm.prank(node.addr);
        angstrom.execute("");

        assertEq(angstrom.lastBlockUpdated(), block2);
    }

    function test_fuzzing_unlockWithEmptyAttestation(address submitter, uint256 bn) public {
        uint64 unlock_block = bound_block(bn);

        bytes32 digest = erc712Hash(
            computeDomainSeparator(address(angstrom)),
            keccak256(
                abi.encode(keccak256("AttestAngstromBlockEmpty(uint64 block_number)"), unlock_block)
            )
        );

        (uint8 v, bytes32 r, bytes32 s) = vm.sign(node.key, digest);

        vm.roll(unlock_block);
        vm.prank(submitter);
        angstrom.unlockWithEmptyAttestation(node.addr, abi.encodePacked(r, s, v));

        assertEq(angstrom.lastBlockUpdated(), unlock_block);
    }

    function test_fuzzing_swapWithUnlockData(
        uint256 bnInput,
        uint24 unlockedFee,
        uint256 swapAmount1,
        uint256 swapAmount2
    ) public {
        uint64 bn = bound_block(bnInput);
        vm.roll(bn);

        bytes32 digest = erc712Hash(
            computeDomainSeparator(address(angstrom)),
            keccak256(abi.encode(keccak256("AttestAngstromBlockEmpty(uint64 block_number)"), bn))
        );
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(node.key, digest);

        bytes memory unlockData = bytes.concat(bytes20(node.addr), r, s, bytes1(v));

        unlockedFee = uint24(bound(unlockedFee, 0.01e6, 1.0e6));
        swapAmount1 = bound(swapAmount1, 1e8, 10e18);
        swapAmount2 = bound(swapAmount2, 1e8, 10e18);

        uint248 liq = 100_000e21;

        PoolKey memory pk = _createPool(60, unlockedFee, liq);

        vm.prank(node.addr);
        angstrom.execute("");
        int128 withFeeOut =
            actor.swap(pk, true, -int256(swapAmount1), 4295128740, unlockData).amount1();

        assertGe(withFeeOut, 0);
        assertEq(angstrom.lastBlockUpdated(), bn);

        withFeeOut = actor.swap(pk, true, -int256(swapAmount2), 4295128740, unlockData).amount1();
        assertGe(withFeeOut, 0);
    }

    function _createPool(uint16 tickSpacing, uint24 unlockedFee, uint248 startLiquidity)
        internal
        returns (PoolKey memory pk)
    {
        vm.prank(controller);
        angstrom.configurePool(asset0, asset1, tickSpacing, 0, unlockedFee);
        angstrom.initializePool(asset0, asset1, 0, TickMath.getSqrtPriceAtTick(0));
        int24 spacing = int24(uint24(tickSpacing));
        pk = poolKey(angstrom, asset0, asset1, spacing);
        if (startLiquidity > 0) {
            actor.modifyLiquidity(
                pk, -1 * spacing, 1 * spacing, int256(uint256(startLiquidity)), bytes32(0)
            );
        }

        return pk;
    }

    function bound_block(uint256 bn) internal pure returns (uint64) {
        return bound_block(bn, type(uint64).max);
    }

    function bound_block(uint256 bn, uint64 upper) internal pure returns (uint64) {
        return uint64(bound(bn, 1, upper));
    }

    function digest712(bytes32 structHash) internal view returns (bytes32) {
        return erc712Hash(domainSeparator, structHash);
    }
}
