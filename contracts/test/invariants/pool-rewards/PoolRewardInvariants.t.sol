// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "test/_helpers/BaseTest.sol";
import {POOL_FEE, ANGSTROM_HOOK_FLAGS} from "src/Constants.sol";
import {PoolRewardsHandler} from "./PoolRewardsHandler.sol";

import {PoolGate} from "test/_helpers/PoolGate.sol";
import {TickMath} from "v4-core/src/libraries/TickMath.sol";

import {PRNG} from "super-sol/collections/PRNG.sol";
import {MockERC20} from "super-sol/mocks/MockERC20.sol";
import {ExtAngstrom} from "test/_view-ext/ExtAngstrom.sol";
import {UniV4Inspector} from "test/_view-ext/UniV4Inspector.sol";
import {TickLib} from "src/libraries/TickLib.sol";
import {TickReward, RewardLib} from "test/_helpers/RewardLib.sol";
import {UsedIndexMap} from "super-sol/collections/UsedIndexMap.sol";
import {PoolKey} from "v4-core/src/types/PoolKey.sol";
import {PoolId, PoolIdLibrary} from "v4-core/src/types/PoolId.sol";

int24 constant TICK_SPACING = 60;

/// @author philogy <https://github.com/philogy>
contract PoolRewardsInvariantTest is BaseTest {
    using TickMath for int24;

    UniV4Inspector public uniV4;
    ExtAngstrom public angstrom;
    PoolGate public gate;
    PoolId public id;
    PoolId public refId;

    address public asset0;
    address public asset1;
    address public owner = makeAddr("owner");
    address public controller = makeAddr("controller");

    PoolRewardsHandler handler;
    bytes4[] handlerSelectors;

    function setUp() public {
        (asset0, asset1) = deployTokensSorted();

        vm.prank(owner);
        uniV4 = new UniV4Inspector();
        gate = new PoolGate(address(uniV4));
        gate.tickSpacing(TICK_SPACING);

        int24 startTick = 0;
        refId = PoolIdLibrary.toId(poolKey(address(asset0), address(asset1), TICK_SPACING));
        gate.setHook(address(0));
        gate.initializePool(address(asset0), address(asset1), startTick.getSqrtPriceAtTick(), 0);

        angstrom = ExtAngstrom(deployAngstrom(type(ExtAngstrom).creationCode, uniV4, controller));
        id = PoolIdLibrary.toId(poolKey());

        vm.prank(controller);
        angstrom.configurePool(asset0, asset1, uint16(uint24(TICK_SPACING)), 0);

        gate.setHook(address(angstrom));
        gate.initializePool(address(asset0), address(asset1), startTick.getSqrtPriceAtTick(), 0);

        handler =
            new PoolRewardsHandler(uniV4, angstrom, gate, id, refId, asset0, asset1, controller);

        handlerSelectors.push(PoolRewardsHandler.rewardLiquidity.selector);
        handlerSelectors.push(PoolRewardsHandler.swapToPrice.selector);
        handlerSelectors.push(PoolRewardsHandler.swapToBoundary.selector);

        targetSelector(FuzzSelector({addr: address(handler), selectors: handlerSelectors}));
        targetContract(address(handler));

        handler.addLiquidity(address(this), -2 * TICK_SPACING, startTick, 1e21);
        handler.addLiquidity(address(this), startTick, 2 * TICK_SPACING, 1e21);
        handler.addLiquidity(address(this), -3 * TICK_SPACING, 1 * TICK_SPACING, 0.0083e21);
        handler.addLiquidity(address(this), -10 * TICK_SPACING, -9 * TICK_SPACING, 0.83e21);
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
                    totalReward +=
                        reward.amount * pos.liquidity / handler.ghost_liquidityAtTick(reward.tick);
                }
            }
            uint256 computed = angstrom.positionRewards(
                id, address(handler), pos.lowerTick, pos.upperTick, bytes32(0), u128(pos.liquidity)
            );
            assertApproxEqRel(totalReward, computed, 0.000001e18);
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

    function invariant_rewardsDistributedWell_17() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_18() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_19() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_20() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_21() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_22() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_23() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_24() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_25() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_26() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_27() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_28() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_29() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_30() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_31() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_32() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_33() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_34() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_35() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_36() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_37() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_38() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_39() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_40() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_41() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_42() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_43() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_44() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_45() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_46() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_47() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_48() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_49() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_50() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_51() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_52() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_53() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_54() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_55() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_56() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_57() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_58() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_59() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_60() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_61() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_62() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_63() public view {
        invariant_rewardsDistributedWell_1();
    }

    function invariant_rewardsDistributedWell_64() public view {
        invariant_rewardsDistributedWell_1();
    }

    function poolKey() internal view returns (PoolKey memory) {
        return poolKey(angstrom, asset0, asset1, TICK_SPACING);
    }
}
