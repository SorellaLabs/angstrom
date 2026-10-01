// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {Angstrom, UnlockHook} from "src/Angstrom.sol";

import {PoolManager} from "v4-core/src/PoolManager.sol";
import {TickMath} from "v4-core/src/libraries/TickMath.sol";
import {Hooks, IHooks} from "v4-core/src/libraries/Hooks.sol";
import {CustomRevert} from "v4-core/src/libraries/CustomRevert.sol";
import {BalanceDelta} from "v4-core/src/types/BalanceDelta.sol";
import {MAX_UNLOCK_FEE_E6} from "src/modules/TopLevelAuth.sol";

import {BaseTest} from "test/_helpers/BaseTest.sol";
import {RouterActor, PoolKey} from "test/_mocks/RouterActor.sol";
import {MockERC20} from "super-sol/mocks/MockERC20.sol";

import {console} from "forge-std/console.sol";

/// @author philogy <https://github.com/philogy>
contract UnlookHookTest is BaseTest {
    Angstrom angstrom;
    PoolManager uni;

    address asset0;
    address asset1;

    address controller = makeAddr("controller");
    address node = makeAddr("the_one_node");

    RouterActor actor;

    function setUp() public {
        uni = new PoolManager(address(0));
        angstrom = Angstrom(deployAngstrom(type(Angstrom).creationCode, uni, controller));
        (asset0, asset1) = deployTokensSorted();
        vm.prank(controller);
        angstrom.toggleNodes(addressArray(abi.encode(node)));

        actor = new RouterActor(uni);

        MockERC20(asset0).mint(address(actor), 100_000_000e18);
        MockERC20(asset1).mint(address(actor), 100_000_000e18);
    }

    function test_fuzzing_preventsSwappingBeforeUnlock(uint32 bn) public {
        vm.roll(boundBlock(bn));

        // Create pool.
        vm.prank(controller);
        angstrom.configurePool(asset0, asset1, 60, 0, 0, 0);
        angstrom.initializePool(asset0, asset1, 0, TickMath.getSqrtPriceAtTick(0));
        PoolKey memory pk = poolKey(angstrom, asset0, asset1, 60);
        actor.modifyLiquidity(pk, -60, 60, 100_000e21, bytes32(0));

        vm.expectRevert(
            abi.encodeWithSelector(
                CustomRevert.WrappedError.selector,
                address(angstrom),
                IHooks.beforeSwap.selector,
                bytes.concat(UnlockHook.CannotSwapWhileLocked.selector),
                bytes.concat(Hooks.HookCallFailed.selector)
            )
        );
        actor.swap(pk, true, 1e18, 4295128740);
    }

    function test_fuzzing_swapAfterUnlockZeroForOne(
        uint32 bn,
        uint24 unlockedFee,
        uint256 swapAmount
    ) public {
        vm.roll(boundBlock(bn));

        unlockedFee = uint24(bound(unlockedFee, 0.01e6, MAX_UNLOCK_FEE_E6));
        swapAmount = bound(swapAmount, 1e8, 10e18);

        // ------ PRE SNAPSHOT ------
        uint256 snapshotId = vm.snapshotState();
        uint248 liq = 100_000e21;
        PoolKey memory pk = _createPool(60, 0, liq);

        vm.prank(node);
        angstrom.execute("");
        int128 noFeeOut = actor.swap(pk, true, -int256(swapAmount), 4295128740).amount1();

        vm.revertToState(snapshotId);

        // ------ ACTUAL CALL ------
        pk = _createPool(60, unlockedFee, liq);

        vm.prank(node);
        angstrom.execute("");
        int128 withFeeOut = actor.swap(pk, true, -int256(swapAmount), 4295128740).amount1();

        assertGe(noFeeOut, 0);
        assertGe(withFeeOut, 0);

        uint256 out = uint256(uint128(noFeeOut));
        assertApproxEqAbs(
            (out * (1e6 - unlockedFee)) / 1e6, uint256(uint128(withFeeOut)), out / 1e6
        );
    }

    function test_fuzzing_swapAfterUnlockOneForZero(
        uint32 bn,
        uint24 unlockedFee,
        uint256 swapAmount
    ) public {
        vm.roll(boundBlock(bn));

        unlockedFee = uint24(bound(unlockedFee, 0.01e6, MAX_UNLOCK_FEE_E6));
        swapAmount = bound(swapAmount, 1e8, 10e18);

        // ------ PRE SNAPSHOT ------
        uint256 snapshotId = vm.snapshotState();
        uint248 liq = 100_000e21;
        PoolKey memory pk = _createPool(60, 0, liq);

        vm.prank(node);
        angstrom.execute("");
        int128 noFeeOut = actor.swap(
                pk, false, -int256(swapAmount), 1461446703485210103287273052203988822378723970341
            ).amount0();

        vm.revertToState(snapshotId);

        // ------ ACTUAL CALL ------
        pk = _createPool(60, unlockedFee, liq);

        vm.prank(node);
        angstrom.execute("");
        int128 withFeeOut = actor.swap(
                pk, false, -int256(swapAmount), 1461446703485210103287273052203988822378723970341
            ).amount0();

        assertGe(noFeeOut, 0);
        assertGe(withFeeOut, 0);

        uint256 out = uint256(uint128(noFeeOut));
        assertApproxEqAbs(
            (out * (1e6 - unlockedFee)) / 1e6, uint256(uint128(withFeeOut)), out / 1e6
        );
    }

    function _createPool(uint16 tickSpacing, uint24 unlockedFee, uint248 startLiquidity)
        internal
        returns (PoolKey memory pk)
    {
        vm.prank(controller);
        angstrom.configurePool(asset0, asset1, tickSpacing, 0, unlockedFee, 0);
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

    function boundBlock(uint32 bn) internal pure returns (uint32) {
        return uint32(bound(bn, 1, type(uint32).max));
    }
}
