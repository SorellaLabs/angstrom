// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "test/_helpers/BaseTest.sol";
import {TICK_SPACING} from "src/Constants.sol";
import {PoolRewardsHandler} from "./PoolRewardsHandler.sol";

import {PoolGate} from "test/_helpers/PoolGate.sol";
import {TickMath} from "v4-core/src/libraries/TickMath.sol";

import {PoolId} from "v4-core/src/types/PoolId.sol";
import {PRNG} from "super-sol/collections/PRNG.sol";
import {MockERC20} from "super-sol/mocks/MockERC20.sol";
import {ExtAngstrom} from "../../_view-ext/ExtAngstrom.sol";
import {UniV4Inspector} from "test/_view-ext/UniV4Inspector.sol";
import {TickLib} from "src/libraries/TickLib.sol";
import {TickReward, RewardLib} from "test/_helpers/RewardLib.sol";
import {UsedIndexMap} from "super-sol/collections/UsedIndexMap.sol";
import {PoolKey} from "v4-core/src/types/PoolKey.sol";
import {PoolId, PoolIdLibrary} from "v4-core/src/types/PoolId.sol";
import {ConversionLib} from "src/libraries/ConversionLib.sol";
import {HookDeployer} from "test/_helpers/HookDeployer.sol";

import {console} from "forge-std/console.sol";

/// @author philogy <https://github.com/philogy>
contract PoolRewardsInvariantTest is BaseTest, HookDeployer {
    using TickMath for int24;

    UniV4Inspector public uniV4;
    ExtAngstrom public angstrom;
    PoolGate public gate;
    PoolId public id;

    MockERC20 public asset0 = new MockERC20();
    MockERC20 public asset1 = new MockERC20();
    address public owner = makeAddr("owner");
    address public gov = makeAddr("gov");

    PoolRewardsHandler handler;
    bytes4[] handlerSelectors;

    function setUp() public {
        if (asset1 < asset0) (asset0, asset1) = (asset1, asset0);

        vm.prank(owner);
        uniV4 = new UniV4Inspector();
        gate = new PoolGate(address(uniV4));

        (bool success, address angstromAddr,) = deployHook(
            abi.encodePacked(type(ExtAngstrom).creationCode, abi.encode(uniV4, gov)), _angstromFlags(), CREATE2_FACTORY
        );
        gate.setHook(angstromAddr);

        assertTrue(success, "Failed to deploy angstrom");
        angstrom = ExtAngstrom(angstromAddr);
        id = PoolIdLibrary.toId(poolKey());

        int24 startTick = 0;
        gate.initializePool(address(asset0), address(asset1), startTick.getSqrtPriceAtTick());

        handler = new PoolRewardsHandler(uniV4, angstrom, gate, id, asset0, asset1, gov);

        handlerSelectors.push(PoolRewardsHandler.rewardLiquidity.selector);

        targetSelector(FuzzSelector({addr: address(handler), selectors: handlerSelectors}));
        targetContract(address(handler));

        handler.addLiquidity(-2 * TICK_SPACING, startTick, 1e21);
        handler.addLiquidity(startTick, 2 * TICK_SPACING, 1e21);
        handler.addLiquidity(-3 * TICK_SPACING, 1 * TICK_SPACING, 0.0083e21);
        handler.addLiquidity(-10 * TICK_SPACING, -9 * TICK_SPACING, 0.83e21);
    }

    function invariant_rewardsDistributedWell_1() public view {
        PoolRewardsHandler.Position[] memory positions = handler.ghost_positions();
        TickReward[] memory rewards = handler.ghost_tickRewards();

        for (uint256 i = 0; i < positions.length; i++) {
            PoolRewardsHandler.Position memory pos = positions[i];
            uint256 totalReward = 0;
            for (uint256 j = 0; j < rewards.length; j++) {
                TickReward memory reward = rewards[j];
                if (pos.lowerTick <= reward.tick && reward.tick < pos.upperTick) {
                    totalReward += reward.amount * pos.liquidity / handler.ghost_liquidityAtTick(reward.tick);
                }
            }
            assertApproxEqRel(
                totalReward,
                angstrom.positionRewardGrowth(id, pos.lowerTick, pos.upperTick, u128(pos.liquidity)),
                0.000001e18
            );
        }
    }

    function invariant_rewardsDistributedWell_2() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_3() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_4() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_5() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_6() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_7() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_8() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_9() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_10() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_11() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_12() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_13() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_14() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_15() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_16() public view {
        invariant_rewardsDistributedWell_1();
    }

    function poolKey() internal view returns (PoolKey memory) {
        return ConversionLib.toPoolKey(address(angstrom), address(asset0), address(asset1));
    }
}
