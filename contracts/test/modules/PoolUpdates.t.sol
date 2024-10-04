// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "test/_helpers/BaseTest.sol";
import {PoolRewardsHandler} from "../invariants/pool-rewards/PoolRewardsHandler.sol";
import {POOL_FEE, ANGSTROM_HOOK_FLAGS} from "src/Constants.sol";

import {PoolKey} from "v4-core/src/types/PoolKey.sol";
import {PoolId, PoolIdLibrary} from "v4-core/src/types/PoolId.sol";
import {MockERC20} from "super-sol/mocks/MockERC20.sol";
import {TickMath} from "v4-core/src/libraries/TickMath.sol";
import {UniV4Inspector} from "test/_view-ext/UniV4Inspector.sol";
import {ExtAngstrom} from "test/_view-ext/ExtAngstrom.sol";
import {PoolGate} from "test/_helpers/PoolGate.sol";
import {HookDeployer} from "test/_helpers/HookDeployer.sol";
import {Position} from "v4-core/src/libraries/Position.sol";

import {TickReward, RewardLib} from "test/_helpers/RewardLib.sol";
import {console} from "forge-std/console.sol";

int24 constant TICK_SPACING = 60;

/// @author philogy <https://github.com/philogy>
contract PoolUpdatesTest is HookDeployer, BaseTest {
    using TickMath for int24;

    UniV4Inspector public uniV4;
    ExtAngstrom public angstrom;
    PoolGate public gate;
    PoolId public id;
    PoolId public refId;

    address public asset0;
    address public asset1;
    address public uniOwner = makeAddr("uniOwner");
    address public gov = makeAddr("gov");

    PoolRewardsHandler handler;

    function setUp() public {
        (asset0, asset1) = deployTokensSorted();

        vm.prank(uniOwner);
        uniV4 = new UniV4Inspector();
        gate = new PoolGate(address(uniV4));

        int24 startTick = 0;
        refId = PoolIdLibrary.toId(poolKey(asset0, asset1, TICK_SPACING));
        gate.setHook(address(0));
        gate.initializePool(address(asset0), address(asset1), startTick.getSqrtPriceAtTick(), 0);

        angstrom = ExtAngstrom(deployAngstrom(type(ExtAngstrom).creationCode, uniV4, gov));
        id = PoolIdLibrary.toId(poolKey());

        vm.prank(gov);
        angstrom.configurePool(address(asset0), address(asset1), uint16(uint24(TICK_SPACING)), 0);

        gate.setHook(address(angstrom));
        gate.initializePool(address(asset0), address(asset1), startTick.getSqrtPriceAtTick(), 0);

        handler = new PoolRewardsHandler(uniV4, angstrom, gate, id, refId, asset0, asset1, gov);
    }

    function test_addOverExistingPosition() public {
        console.log("1");
        address lp = makeAddr("lp");
        uint128 liq1 = 1e21;
        handler.addLiquidity(lp, -180, 180, liq1);

        assertEq(positionRewards(lp, -180, 180, liq1), 0);

        console.log("2");
        uint128 amount1 = 23.872987e18;
        bumpBlock();
        handler.rewardTicks(re(TickReward({tick: -180, amount: amount1})));

        assertEq(positionRewards(lp, -180, 180, liq1), amount1);

        console.log("3");
        uint128 liq2 = 1.5e21;
        handler.addLiquidity(lp, -180, 180, liq2);
        assertEq(positionRewards(lp, -180, 180, liq1 + liq2), amount1);

        console.log("4");
        uint128 amount2 = 4.12e18;
        bumpBlock();
        handler.rewardTicks(re(TickReward({tick: -180, amount: amount2})));

        assertEq(positionRewards(lp, -180, 180, liq1 + liq2), amount1 + amount2);
    }

    function test_addInSubordinateRange() public {
        uint128 liq1 = 8.2e21;
        address lp1 = makeAddr("lp_1");
        handler.addLiquidity(lp1, -180, 180, liq1);

        uint128 amount1 = 4.0e18;
        bumpBlock();
        handler.rewardTicks(re(TickReward({tick: -180, amount: amount1})));
        assertApproxEqRel(
            positionRewards(lp1, -180, 180, liq1), amount1, 1.0e18 / 1e12, "reward while alone"
        );

        uint128 liq2 = 0.64e21;
        address lp2 = makeAddr("lp_2");
        handler.addLiquidity(lp2, -60, 60, liq2);
        assertEq(positionRewards(lp2, -60, 60, liq2), 0, "lp2 rewards not 0");

        uint128 amount2 = 2.3e18;
        bumpBlock();
        handler.rewardTicks(re(TickReward({tick: -60, amount: amount2})));

        uint128 totalLiq = liq1 + liq2;
        assertApproxEqRel(
            positionRewards(lp1, -180, 180, liq1),
            amount1 + uint256(amount2) * liq1 / totalLiq,
            1.0e18 / 1e12,
            "lp1"
        );
        assertApproxEqRel(
            positionRewards(lp2, -60, 60, liq2),
            uint256(amount2) * liq2 / totalLiq,
            1.0e18 / 1e12,
            "lp2"
        );

        uint128 liq3 = 64.64e21;
        address lp3 = makeAddr("lp_3");
        handler.addLiquidity(lp3, -60, 60, liq3);
        assertApproxEqRel(
            positionRewards(lp1, -180, 180, liq1),
            amount1 + uint256(amount2) * liq1 / totalLiq,
            1.0e18 / 1e12,
            "lp1"
        );
        assertApproxEqRel(
            positionRewards(lp2, -60, 60, liq2),
            uint256(amount2) * liq2 / totalLiq,
            1.0e18 / 1e12,
            "lp2"
        );
        assertEq(positionRewards(lp3, -60, 60, liq3), 0, "lp3 rewards not starting at 0");

        uint128 amount3 = 34.0287e18;
        bumpBlock();
        handler.rewardTicks(re(TickReward({tick: -180, amount: amount3})));
        assertApproxEqRel(
            positionRewards(lp1, -180, 180, liq1),
            amount1 + amount3 + uint256(amount2) * liq1 / totalLiq,
            1.0e18 / 1e12,
            "lp1"
        );
        assertApproxEqRel(
            positionRewards(lp2, -60, 60, liq2),
            uint256(amount2) * liq2 / totalLiq,
            1.0e18 / 1e12,
            "lp2"
        );
        assertEq(positionRewards(lp3, -60, 60, liq3), 0, "lp3 rewards not kept at 0");
    }

    function test_addAndRemove_simple() public {
        uint128 liq1 = 8.2e21;
        address lp1 = makeAddr("lp_1");
        handler.addLiquidity(lp1, -180, 180, liq1);

        uint128 amount = 1006.87299e18;
        bumpBlock();
        handler.rewardTicks(re(TickReward({tick: -180, amount: amount})));
        assertApproxEqRel(
            positionRewards(lp1, -180, 180, liq1), amount, 1.0e18 / 1e12, "reward while alone"
        );

        vm.startPrank(lp1);
        gate.setHook(address(angstrom));
        (uint256 amount0Out, uint256 amount1Out) = removeLiquidity(-180, 180, 0);
        vm.stopPrank();
        assertApproxEqRel(amount0Out, amount, 1.0e18 / 1e12, "no reward");
        assertEq(amount1Out, 0, "got something from amount1");
    }

    function positionRewards(address owner, int24 lowerTick, int24 upperTick, uint128 liquidity)
        internal
        view
        returns (uint256)
    {
        return angstrom.positionRewards(id, owner, lowerTick, upperTick, bytes32(0), liquidity);
    }

    function removeLiquidity(int24 lowerTick, int24 upperTick, uint256 liquidity)
        internal
        returns (uint256, uint256)
    {
        return gate.removeLiquidity(
            address(asset0), address(asset1), lowerTick, upperTick, liquidity, bytes32(0)
        );
    }

    function poolKey() internal view returns (PoolKey memory) {
        return poolKey(angstrom, asset0, asset1, TICK_SPACING);
    }

    function re(TickReward memory reward) internal pure returns (TickReward[] memory r) {
        r = new TickReward[](1);
        r[0] = reward;
    }

    function re(TickReward memory r1, TickReward memory r2)
        internal
        pure
        returns (TickReward[] memory r)
    {
        r = new TickReward[](2);
        r[0] = r1;
        r[1] = r2;
    }

    function re(TickReward memory r1, TickReward memory r2, TickReward memory r3)
        internal
        pure
        returns (TickReward[] memory r)
    {
        r = new TickReward[](3);
        r[0] = r1;
        r[1] = r2;
        r[2] = r3;
    }
}
