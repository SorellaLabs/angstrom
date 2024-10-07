// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "test/_helpers/BaseTest.sol";
import {ExtAngstrom} from "test/_view-ext/ExtAngstrom.sol";
import {TopLevelAuth} from "src/modules/TopLevelAuth.sol";
import {
    PoolConfigStore,
    MAX_FEE,
    STORE_HEADER_SIZE,
    PoolConfigStoreLib,
    StoreKey
} from "src/libraries/PoolConfigStore.sol";
import {ENTRY_SIZE} from "src/types/ConfigEntry.sol";
import {IPoolManager} from "v4-core/src/interfaces/IPoolManager.sol";
import {console} from "forge-std/console.sol";

/// @author philogy <https://github.com/philogy>
contract PoolConfigStoreTest is BaseTest {
    ExtAngstrom angstrom;
    address controller;

    uint256 constant TOTAL_ASSETS = 32;
    address[TOTAL_ASSETS] assets;

    function setUp() public {
        controller = makeAddr("controller");
        angstrom = ExtAngstrom(
            deployAngstrom(type(ExtAngstrom).creationCode, IPoolManager(address(0)), controller)
        );

        assets[0] = makeAddr("asset_0");
        for (uint256 i = 1; i < TOTAL_ASSETS; i++) {
            assets[i] = addrInc(assets[i - 1]);
        }
    }

    function test_default_store() public view {
        PoolConfigStore store = angstrom.configStore();
        assertEq(store.totalEntries(), 0);
    }

    function test_configure_1() public {
        vm.prank(controller);
        angstrom.configurePool(assets[0], assets[1], 12, 0.01e6);

        PoolConfigStore store = angstrom.configStore();
        assertEq(store.totalEntries(), 1);
        (int24 tickSpacing, uint24 feeInE6) = store.get(skey(assets[0], assets[1]), 0);
        assertEq(tickSpacing, 12);
        assertEq(feeInE6, 0.01e6);
    }

    function test_configure_newOnly() public {
        vm.prank(controller);
        angstrom.configurePool(assets[0], assets[1], 19, 0.01e6);
        PoolConfigStore store1 = angstrom.configStore();
        assertEq(store1.totalEntries(), 1);
        (int24 tickSpacing, uint24 feeInE6) = store1.get(skey(assets[0], assets[1]), 0);
        assertEq(tickSpacing, 19);
        assertEq(feeInE6, 0.01e6);

        vm.prank(controller);
        angstrom.configurePool(assets[3], assets[31], 120, 0.000134e6);
        PoolConfigStore store2 = angstrom.configStore();
        assertTrue(PoolConfigStore.unwrap(store1) != PoolConfigStore.unwrap(store2));
        assertEq(store2.totalEntries(), 2);
        (tickSpacing, feeInE6) = store2.get(skey(assets[0], assets[1]), 0);
        assertEq(tickSpacing, 19);
        assertEq(feeInE6, 0.01e6);
        (tickSpacing, feeInE6) = store2.get(skey(assets[3], assets[31]), 1);
        assertEq(tickSpacing, 120);
        assertEq(feeInE6, 0.000134e6);

        vm.prank(controller);
        angstrom.configurePool(assets[4], assets[7], 41, 0.1003e6);
        PoolConfigStore store3 = angstrom.configStore();
        assertTrue(PoolConfigStore.unwrap(store2) != PoolConfigStore.unwrap(store3));
        assertEq(store3.totalEntries(), 3);
        (tickSpacing, feeInE6) = store3.get(skey(assets[0], assets[1]), 0);
        assertEq(tickSpacing, 19);
        assertEq(feeInE6, 0.01e6);
        (tickSpacing, feeInE6) = store3.get(skey(assets[3], assets[31]), 1);
        assertEq(tickSpacing, 120);
        assertEq(feeInE6, 0.000134e6);
        (tickSpacing, feeInE6) = store3.get(skey(assets[4], assets[7]), 2);
        assertEq(tickSpacing, 41);
        assertEq(feeInE6, 0.1003e6);
    }

    function test_fuzzing_removeExistingWhenGreaterThanOne(uint256 indexToRemove) public {
        indexToRemove = bound(indexToRemove, 0, 2);

        vm.startPrank(controller);
        angstrom.configurePool(assets[0], assets[1], 19, 0.01e6);
        angstrom.configurePool(assets[3], assets[31], 120, 0.000134e6);
        angstrom.configurePool(assets[4], assets[7], 41, 0.1003e6);
        vm.stopPrank();

        PoolConfigStore store = angstrom.configStore();
        vm.prank(controller);
        angstrom.removePool(PoolConfigStore.unwrap(store), indexToRemove);
        PoolConfigStore storeAfter = angstrom.configStore();
        assertTrue(PoolConfigStore.unwrap(store) != PoolConfigStore.unwrap(storeAfter));
        assertEq(storeAfter.totalEntries(), 2);
        if (indexToRemove == 0) {
            (int24 tickSpacing, uint24 feeInE6) = storeAfter.get(skey(assets[3], assets[31]), 0);
            assertEq(tickSpacing, 120);
            assertEq(feeInE6, 0.000134e6);
            (tickSpacing, feeInE6) = storeAfter.get(skey(assets[4], assets[7]), 1);
            assertEq(tickSpacing, 41);
            assertEq(feeInE6, 0.1003e6);
        } else if (indexToRemove == 1) {
            (int24 tickSpacing, uint24 feeInE6) = storeAfter.get(skey(assets[0], assets[1]), 0);
            assertEq(tickSpacing, 19);
            assertEq(feeInE6, 0.01e6);
            (tickSpacing, feeInE6) = storeAfter.get(skey(assets[4], assets[7]), 1);
            assertEq(tickSpacing, 41);
            assertEq(feeInE6, 0.1003e6);
        } else if (indexToRemove == 2) {
            (int24 tickSpacing, uint24 feeInE6) = storeAfter.get(skey(assets[0], assets[1]), 0);
            assertEq(tickSpacing, 19);
            assertEq(feeInE6, 0.01e6);
            (tickSpacing, feeInE6) = storeAfter.get(skey(assets[3], assets[31]), 1);
            assertEq(tickSpacing, 120);
            assertEq(feeInE6, 0.000134e6);
        }
    }

    function test_fuzzing_removeStandalone(
        address asset0,
        address asset1,
        uint16 tickSpacing,
        uint24 feeInE6
    ) public {
        vm.assume(asset0 != asset1);
        tickSpacing = uint16(bound(tickSpacing, 1, type(uint16).max));
        feeInE6 = uint24(bound(feeInE6, 0, MAX_FEE));

        vm.prank(controller);
        angstrom.configurePool(asset0, asset1, tickSpacing, feeInE6);
        PoolConfigStore store = angstrom.configStore();

        vm.prank(controller);
        angstrom.removePool(PoolConfigStore.unwrap(store), 0);

        PoolConfigStore newStore = angstrom.configStore();
        assertEq(newStore.totalEntries(), 0);
        assertEq(PoolConfigStore.unwrap(newStore), address(0));
    }

    function test_configure_existing() public {
        vm.prank(controller);
        angstrom.configurePool(assets[0], assets[1], 190, 0);
        PoolConfigStore store1 = angstrom.configStore();
        assertEq(store1.totalEntries(), 1);
        (int24 tickSpacing, uint24 feeInE6) = store1.get(skey(assets[0], assets[1]), 0);
        assertEq(tickSpacing, 190);
        assertEq(feeInE6, 0);

        vm.prank(controller);
        angstrom.configurePool(assets[0], assets[1], 21, 0.199e6);
        PoolConfigStore store2 = angstrom.configStore();
        assertTrue(PoolConfigStore.unwrap(store1) != PoolConfigStore.unwrap(store2));
        assertEq(store2.totalEntries(), 1);

        (tickSpacing, feeInE6) = store2.get(skey(assets[0], assets[1]), 0);
        assertEq(tickSpacing, 21);
        assertEq(feeInE6, 0.199e6);
    }

    function test_fuzzing_prevents_nonControllerConfiguring(
        address configurer,
        address asset0,
        address asset1,
        uint16 tickSpacing,
        uint24 feeInE6
    ) public {
        vm.assume(configurer != controller);
        vm.assume(asset0 != asset1);
        tickSpacing = uint16(bound(tickSpacing, 1, type(uint16).max));
        feeInE6 = uint24(bound(feeInE6, 0, MAX_FEE));
        vm.prank(configurer);
        vm.expectRevert(TopLevelAuth.NotController.selector);
        if (asset0 > asset1) (asset0, asset1) = (asset1, asset0);
        angstrom.configurePool(asset0, asset1, tickSpacing, feeInE6);
    }

    function test_fuzzing_prevents_providingDuplicate(
        address asset,
        uint16 tickSpacing,
        uint24 feeInE6
    ) public {
        tickSpacing = uint16(bound(tickSpacing, 1, type(uint16).max));
        feeInE6 = uint24(bound(feeInE6, 0, MAX_FEE));
        vm.prank(controller);
        vm.expectRevert(PoolConfigStoreLib.DuplicateAsset.selector);
        angstrom.configurePool(asset, asset, tickSpacing, feeInE6);
    }

    function test_fuzzing_prevents_providingTickSpacingZero(
        address asset0,
        address asset1,
        uint24 feeInE6
    ) public {
        vm.assume(asset0 != asset1);
        if (asset1 < asset0) (asset0, asset1) = (asset1, asset0);
        feeInE6 = uint24(bound(feeInE6, 0, MAX_FEE));
        vm.prank(controller);
        vm.expectRevert(PoolConfigStoreLib.InvalidTickSpacing.selector);
        angstrom.configurePool(asset0, asset1, 0, feeInE6);
    }

    function test_fuzzing_prevents_settingFeeAboveMax(
        address asset0,
        address asset1,
        uint16 tickSpacing,
        uint24 feeInE6
    ) public {
        vm.assume(asset0 != asset1);
        if (asset1 < asset0) (asset0, asset1) = (asset1, asset0);
        feeInE6 = uint24(bound(feeInE6, MAX_FEE + 1, type(uint24).max));
        tickSpacing = uint16(bound(tickSpacing, 1, type(uint16).max));
        vm.prank(controller);
        vm.expectRevert(PoolConfigStoreLib.FeeAboveMax.selector);
        angstrom.configurePool(asset0, asset1, tickSpacing, feeInE6);
    }

    function addrInc(address prev) internal pure returns (address next) {
        assembly ("memory-safe") {
            mstore(0x00, prev)
            let hash := keccak256(0x00, 0x20)
            next := add(prev, shr(120, hash))
        }
    }

    function skey(address asset0, address asset1) internal pure returns (StoreKey key) {
        assertTrue(asset0 < asset1, "Building key with out of order assets");
        key = PoolConfigStoreLib.keyFromAssetsUnchecked(asset0, asset1);
    }
}
