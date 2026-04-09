// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {AngstromTest} from "test/Angstrom.t.sol";
import {BaseTest} from "test/_helpers/BaseTest.sol";
import {PoolManager} from "v4-core/src/PoolManager.sol";
import {TickMath} from "v4-core/src/libraries/TickMath.sol";
import {PoolId, PoolIdLibrary} from "v4-core/src/types/PoolId.sol";
import {Angstrom} from "src/Angstrom.sol";
import {TopLevelAuth, MAX_UNLOCK_FEE_E6} from "src/modules/TopLevelAuth.sol";
import {Bundle} from "test/_reference/Bundle.sol";
import {Asset, AssetLib} from "test/_reference/Asset.sol";
import {Pair, PairLib} from "test/_reference/Pair.sol";
import {UserOrder, UserOrderLib} from "test/_reference/UserOrder.sol";
import {PartialStandingOrder, ExactFlashOrder} from "test/_reference/OrderTypes.sol";
import {PriceAB as Price10} from "src/types/Price.sol";
import {MockERC20} from "super-sol/mocks/MockERC20.sol";
import {AngstromView} from "src/periphery/AngstromView.sol";
import {RouterActor, PoolKey} from "test/_mocks/RouterActor.sol";
import {SafeTransferLib} from "solady/src/utils/SafeTransferLib.sol";

import {IHooks} from "v4-core/src/interfaces/IHooks.sol";
import {Hooks} from "v4-core/src/libraries/Hooks.sol";

// to force compile
import {IPositionDescriptor} from "v4-periphery/src/interfaces/IPositionDescriptor.sol";
import {console} from "forge-std/console.sol";

import {IUniV4} from "src/interfaces/IUniV4.sol";
import {PoolUpdateLib, PoolUpdate, RewardsUpdate} from "test/_reference/PoolUpdate.sol";
import {PoolConfigStore} from "src/libraries/PoolConfigStore.sol";

contract MainnetForkTest is AngstromTest {
    using AngstromView for Angstrom;
    using SafeTransferLib for address;

    using Hooks for IHooks;

    using PairLib for Pair[];
    using AssetLib for Asset[];

    using IUniV4 for PoolManager;
    using PoolUpdateLib for PoolUpdate;

    struct ModifyLiquidityParams {
        // the lower and upper tick of the position
        int24 tickLower;
        int24 tickUpper;
        // how to modify the liquidity
        int256 liquidityDelta;
        // a value to set if you want unique liquidity positions at the same range
        bytes32 salt;
    }

    address constant WETH = 0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2;
    address constant USDC = 0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48;

    function setUp() public override {
        // fork at specific block
        uint256 forkId = vm.createSelectFork("mainnet", 24831188);
        assertEq(vm.activeFork(), forkId);

        uni = PoolManager(address(0x000000000004444c5dc75cB358380D2e3dE08A90));
        angstrom = Angstrom(0x0000000aa232009084Bd71A5797d089AA4Edfad4);
        domainSeparator = computeDomainSeparator(address(angstrom));
        controller = angstrom.controller();

        vm.prank(controller);
        angstrom.toggleNodes(addressArray(abi.encode(node.addr)));

        actor = new RouterActor(uni);

        (asset0, asset1) = deployTokensSorted();
        MockERC20(asset0).mint(address(uni), 100_000e18);
        MockERC20(asset1).mint(address(uni), 100_000e18);

        MockERC20(asset0).mint(address(actor), 100_000_000e18);
        MockERC20(asset1).mint(address(actor), 100_000_000e18);

        vm.roll(block.number + 1);
    }

    // get the WETH/USDC pool global rewards growth and check that it is nonzero
    function test_getGlobalRewardsGrowth() public {
        PoolId id = PoolId.wrap(
            bytes32(0xe500210c7ea6bfd9f69dce044b09ef384ec2b34832f132baec3b418208e3a657)
        );
        uint256 poolRewardsGlobalGrowth = angstrom.poolRewardsGlobalGrowth({id: id});
        emit log_named_uint("poolRewardsGlobalGrowth", poolRewardsGlobalGrowth);
    }

    function test_claimRewards() public {
        // emulate claim in https://etherscan.io/tx/0x3770e2e8caa1a106fd3e67b60a221815b30122d330e7663a17a09e18f001cef7
        // // fork at specific block
        // uint256 forkId = vm.createSelectFork("mainnet", 24831188);
        // assertEq(vm.activeFork(), forkId);

        // vm.roll(block.number + 1);
        // emit log_named_uint("block.number", block.number);
        // setAngstromLastBlockUpdated(uint64(block.number));

        address user = 0x35b9571eF5eAA24EC626ACdDDE3B02B595A5eB0B;
        address uniPositionManager = 0xbD216513d74C8cf14cf4747E6AaA6420FF64ee9e;

        int24 spacing = 10;
        uint256 tokenId = 192703;
        PoolKey memory pk = poolKey(angstrom, USDC, WETH, spacing);
        uint256 pendingRewards = angstrom.pendingRewards({
            account: uniPositionManager,
            key: pk,
            tickLower: 199320,
            tickUpper: 201320,
            salt: bytes32(tokenId),
            uniV4: uni
        });

        emit log_named_uint("pendingRewards", pendingRewards);
        // pending rewards should be 226319524 at this block
        assertGt(pendingRewards, 0, "pendingRewards should be nonzero");

        uint256 existingLiquidity = uni.getPoolLiquidity(pk.toId());
        uint256 asset0BalanceBefore = MockERC20(USDC).balanceOf(address(user));

        uint256 deadline = 1775606188;
        bytes memory callData;
        {
            bytes memory actions = abi.encodePacked(bytes1(uint8(0x01)));

            // (uint256 tokenId, uint256 liquidity, uint128 amount0Min, uint128 amount1Min, bytes calldata hookData) =
            //             parameters.decodeModifyLiquidityParams();
            uint256 liquidity = 0;
            uint128 amount0Min = 0;
            uint128 amount1Min = 0;
            bytes memory hookData;
            bytes[] memory parameters = new bytes[](1);
            parameters[0] = abi.encode(tokenId, liquidity, amount0Min, amount1Min, hookData);
            // (bytes calldata actions, bytes[] calldata parameters) = data.decodeActionsRouterParams()

            bytes memory unlockData = abi.encode(actions, parameters);
            callData = abi.encodeWithSignature("modifyLiquidities(bytes,uint256)", unlockData, deadline);
        }

        vm.prank(user);
        (bool success, ) = address(uniPositionManager).call(callData);
        require(success, "mocked call failed");

/**
        vm.prank(user);
        // bytes memory callData =
        callData =
        //     abi.encodeWithSignature("modifyLiquidities(bytes,uint256)", unlockData, deadline);
            // unmodified
            // "0xdd46508f00000000000000000000000000000000000000000000000000000000000000400000000000000000000000000000000000000000000000000000000069d599ac0000000000000000000000000000000000000000000000000000000000000260000000000000000000000000000000000000000000000000000000000000004000000000000000000000000000000000000000000000000000000000000000800000000000000000000000000000000000000000000000000000000000000002011100000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000040000000000000000000000000000000000000000000000000000000000000014000000000000000000000000000000000000000000000000000000000000000e0000000000000000000000000000000000000000000000000000000000002f0bf000000000000000000000000000000000000000000000000000ee04f601baa56000000000000000000000000000000000000000000000000000000045c14858e000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000a000000000000000000000000000000000000000000000000000000000000000140000000aa232009084bd71a5797d089aa4edfad40000000000000000000000000000000000000000000000000000000000000000000000000000000000000060000000000000000000000000a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48000000000000000000000000c02aaa39b223fe8d0a0e5c4f27ead9083c756cc20000000000000000000000000000000000000000000000000000000000000001";
        //     // modified
            "0xdd46508f00000000000000000000000000000000000000000000000000000000000000400000000000000000000000000000000000000000000000000000000069d599ac0000000000000000000000000000000000000000000000000000000000000260000000000000000000000000000000000000000000000000000000000000004000000000000000000000000000000000000000000000000000000000000000800000000000000000000000000000000000000000000000000000000000000002011100000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000040000000000000000000000000000000000000000000000000000000000000014000000000000000000000000000000000000000000000000000000000000000e0000000000000000000000000000000000000000000000000000000000002f0bf0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000045c14858e000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000a000000000000000000000000000000000000000000000000000000000000000140000000aa232009084bd71a5797d089aa4edfad40000000000000000000000000000000000000000000000000000000000000000000000000000000000000060000000000000000000000000a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48000000000000000000000000c02aaa39b223fe8d0a0e5c4f27ead9083c756cc20000000000000000000000000000000000000000000000000000000000000001";
        (success, ) = uniPositionManager.call(callData);
        require(success, "mocked call 2 failed");
**/
        assertEq(
            uni.getPoolLiquidity(pk.toId()), existingLiquidity, "call should not modify liquidity"
        );
        uint256 asset0BalanceAfter = MockERC20(USDC).balanceOf(address(user));
        assertEq(
            asset0BalanceAfter - asset0BalanceBefore,
            pendingRewards,
            "pendingRewards amount not equal to transfer amount"
        );
    }

    function test_rewardsClaiming() public {
        uint248 liq = 100_000e21;
        uint248 startLiquidity = liq;
        uint256 bundleFee = 0.002e6;
        uint16 tickSpacing = 60;

        vm.prank(controller);
        angstrom.configurePool({
            asset0: asset0,
            asset1: asset1,
            tickSpacing: tickSpacing,
            bundleFee: uint24(bundleFee),
            unlockedFee: 0,
            protocolUnlockedFee: 0
        });
        angstrom.initializePool({
            assetA: asset0,
            assetB: asset1,
            storeIndex: 2,
            sqrtPriceX96: TickMath.getSqrtPriceAtTick(0)
        });
        int24 spacing = int24(uint24(tickSpacing));
        PoolKey memory pk = poolKey(angstrom, asset0, asset1, spacing);
        if (startLiquidity > 0) {
            actor.modifyLiquidity(
                pk, -1 * spacing, 1 * spacing, int256(uint256(startLiquidity)), bytes32(0)
            );
        }

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

        // MockERC20(asset0).mint(address(angstrom), 0.1e18);
        // uint128 onlyCurrentQuantity = 0;
        uint128[] memory quantities;
        bundle.addPoolUpdate({
            assetIn: asset0,
            assetOut: asset1,
            amountIn: 0,
            onlyCurrent: true,
            onlyCurrentQuantity: 1e18,
            startTick: 0,
            startLiquidity: 0,
            quantities: quantities,
            rewardChecksum: 0
        });

        bundle.assets[0].save += 1.018e18;
        bundle.assets[1].save += 0.218400801603206413e18;
        bundle.assets[0].take += 0;
        bundle.assets[1].take += 10.0e18;
        bundle.assets[1].settle += 10.0e18;

        bundle.assets[0].save -= bundle.poolUpdates[0].rewardUpdate.onlyCurrentQuantity;
        // bundle.assets[0].take += bundle.poolUpdates[0].amountIn;
        uint256 existingLiquidity = uni.getPoolLiquidity(pk.toId());
        emit log_named_uint("existingLiquidity", existingLiquidity);
        // returns 100000000000000000000000000 (as expected)

        emit log(bundle.poolUpdates[0].rewardUpdate.toStr());
        // returns RewardsUpdate::CurrentOnly { amount: 1000000000000000000 } (as expected)

        // manually write rewards slot
        uint256 rewardsMapSlot =
        // uint256(bytes32(keccak256(abi.encode(pk.toId(), POOL_REWARDS_SLOT)))) + REWARD_GROWTH_SIZE;
         uint256(bytes32(keccak256(abi.encode(pk.toId(), 7)))) + 16777216;
        vm.store(address(angstrom), bytes32(rewardsMapSlot), bytes32(uint256(1e30)));

        bytes memory payload = bundle.encode(rawGetConfigStore(address(angstrom)));
        vm.prank(node.addr);
        angstrom.execute(payload);
        // test_userOrderWithFees();

        // int24 tickLower = -1 * int24(uint24(60));
        // int24 tickUpper = 1 * int24(uint24(60));
        uint256 poolRewardsGlobalGrowth = angstrom.poolRewardsGlobalGrowth({id: pk.toId()});
        emit log_named_uint("poolRewardsGlobalGrowth", poolRewardsGlobalGrowth);
        assertGt(poolRewardsGlobalGrowth, 0, "poolRewardsGlobalGrowth should be nonzero");
        {
            uint256 lastGrowthInside = angstrom.getLastGrowthInside({
                id: pk.toId(),
                uniPositionKey: keccak256(
                    abi.encodePacked(
                        address(actor), -1 * int24(uint24(60)), 1 * int24(uint24(60)), bytes32(0)
                    )
                )
            });
            emit log_named_uint("lastGrowthInside", lastGrowthInside);
        }

        // PoolKey memory pk = poolKey(angstrom, asset0, asset1, 60);
        uint256 pendingRewards = angstrom.pendingRewards({
            account: address(actor),
            key: pk,
            tickLower: -1 * int24(uint24(60)),
            tickUpper: 1 * int24(uint24(60)),
            salt: bytes32(0),
            uniV4: uni
        });
        assertGt(pendingRewards, 0, "pendingRewards should be nonzero");
        uint256 asset0BalanceBefore = MockERC20(asset0).balanceOf(address(actor));
        emit log_named_uint("pendingRewards", pendingRewards);

        actor.modifyLiquidity(
            pk, -1 * int24(uint24(60)), 1 * int24(uint24(60)), int256(uint256(0)), bytes32(0)
        );

        {
            uint256 asset0BalanceAfter = MockERC20(asset0).balanceOf(address(actor));
            assertEq(
                asset0BalanceAfter, asset0BalanceBefore + pendingRewards, "pendingRewards incorrect"
            );
        }

        pendingRewards = angstrom.pendingRewards({
            account: address(actor),
            key: pk,
            tickLower: -1 * int24(uint24(60)),
            tickUpper: 1 * int24(uint24(60)),
            salt: bytes32(0),
            uniV4: uni
        });
        assertEq(pendingRewards, 0, "pendingRewards should be zero");
    }

    // override function to allow different store index which is appropriate for mainnet
    function _createPool(
        uint16 tickSpacing,
        uint24 unlockedFee,
        uint248 startLiquidity,
        uint24 protocolUnlockedFee
    ) internal override returns (PoolKey memory pk) {
        vm.prank(controller);
        angstrom.configurePool({
            asset0: asset0,
            asset1: asset1,
            tickSpacing: tickSpacing,
            bundleFee: 0,
            unlockedFee: unlockedFee,
            protocolUnlockedFee: protocolUnlockedFee
        });
        angstrom.initializePool({
            assetA: asset0,
            assetB: asset1,
            storeIndex: 2,
            sqrtPriceX96: TickMath.getSqrtPriceAtTick(0)
        });
        int24 spacing = int24(uint24(tickSpacing));
        pk = poolKey(angstrom, asset0, asset1, spacing);
        if (startLiquidity > 0) {
            actor.modifyLiquidity(
                pk, -1 * spacing, 1 * spacing, int256(uint256(startLiquidity)), bytes32(0)
            );
        }

        return pk;
    }

    function setAngstromLastBlockUpdated(uint64 blockNumber) public {
        uint256 LAST_BLOCK_CONFIG_STORE_SLOT = 3;
        uint256 LAST_BLOCK_BIT_OFFSET = 0;
        uint256 STORE_BIT_OFFSET = 64;
        address configStore = PoolConfigStore.unwrap(angstrom.configStore());
        uint256 slotData = (uint256(blockNumber) << LAST_BLOCK_BIT_OFFSET) | (uint256(uint160(configStore)) << STORE_BIT_OFFSET);
        vm.store(address(angstrom), bytes32(LAST_BLOCK_CONFIG_STORE_SLOT), bytes32(uint256(slotData)));
        uint64 lastBlockUpdated = angstrom.lastBlockUpdated();
        assertEq(blockNumber, lastBlockUpdated, "lastBlockUpdated not updated as expected");
        address configStoreAfter = PoolConfigStore.unwrap(angstrom.configStore());
        assertEq(configStore, configStoreAfter, "configStore should not have changed");
    }
}
