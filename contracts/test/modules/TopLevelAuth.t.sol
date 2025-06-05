// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "test/_helpers/BaseTest.sol";
import {OpenAngstrom} from "test/_mocks/OpenAngstrom.sol";
import {TopLevelAuth, IAngstromAuth, ConfigEntryUpdate} from "src/modules/TopLevelAuth.sol";
import {AngstromView} from "src/periphery/AngstromView.sol";
import {
    PoolConfigStore,
    STORE_HEADER_SIZE,
    PoolConfigStoreLib,
    StoreKey
} from "src/libraries/PoolConfigStore.sol";
import {ConfigEntry, ConfigEntryLib, MAX_FEE} from "src/types/ConfigEntry.sol";
import {ConfigBuffer} from "src/types/ConfigBuffer.sol";
import {ENTRY_SIZE} from "src/types/ConfigEntry.sol";
import {LPFeeLibrary} from "v4-core/src/libraries/LPFeeLibrary.sol";
import {IPoolManager} from "v4-core/src/interfaces/IPoolManager.sol";
import {console} from "forge-std/console.sol";

/// @author philogy <https://github.com/philogy>
contract TopLevelAuthTest is BaseTest {
    using AngstromView for IAngstromAuth;

    OpenAngstrom angstrom;
    address controller;

    uint256 constant TOTAL_ASSETS = 32;
    address[TOTAL_ASSETS] assets;

    function setUp() public {
        controller = makeAddr("controller");
        angstrom = OpenAngstrom(
            deployAngstrom(type(OpenAngstrom).creationCode, IPoolManager(address(0)), controller)
        );

        assets[0] = makeAddr("asset_0");
        for (uint256 i = 1; i < TOTAL_ASSETS; i++) {
            assets[i] = addrInc(assets[i - 1]);
        }
    }

    function test_entry_size() public pure {
        assertEq(
            ENTRY_SIZE,
            32,
            "Ensure that new size doesn't require changes like an index bounds check"
        );
    }

    function test_default_store() public view {
        PoolConfigStore store = angstrom.configStore();
        assertEq(store.totalEntries(), 0);
    }

    function test_configure_1() public {
        vm.prank(controller);
        angstrom.configurePool(assets[0], assets[1], 12, 0.01e6, 0, 0);

        PoolConfigStore store = angstrom.configStore();
        assertEq(store.totalEntries(), 1);
        (int24 tickSpacing, uint24 bundleFee) = store.get(skey(assets[0], assets[1]), 0);
        assertEq(tickSpacing, 12);
        assertEq(bundleFee, 0.01e6);
    }

    function test_configure_newOnly() public {
        vm.prank(controller);
        angstrom.configurePool(assets[0], assets[1], 19, 0.01e6, 0, 0);
        PoolConfigStore store1 = angstrom.configStore();
        assertEq(store1.totalEntries(), 1);
        (int24 tickSpacing, uint24 bundleFee) = store1.get(skey(assets[0], assets[1]), 0);
        assertEq(tickSpacing, 19);
        assertEq(bundleFee, 0.01e6);

        vm.prank(controller);
        angstrom.configurePool(assets[3], assets[31], 120, 0.000134e6, 0, 0);
        PoolConfigStore store2 = angstrom.configStore();
        assertTrue(PoolConfigStore.unwrap(store1) != PoolConfigStore.unwrap(store2));
        assertEq(store2.totalEntries(), 2);
        (tickSpacing, bundleFee) = store2.get(skey(assets[0], assets[1]), 0);
        assertEq(tickSpacing, 19);
        assertEq(bundleFee, 0.01e6);
        (tickSpacing, bundleFee) = store2.get(skey(assets[3], assets[31]), 1);
        assertEq(tickSpacing, 120);
        assertEq(bundleFee, 0.000134e6);

        vm.prank(controller);
        angstrom.configurePool(assets[4], assets[7], 41, 0.1003e6, 0, 0);
        PoolConfigStore store3 = angstrom.configStore();
        assertTrue(PoolConfigStore.unwrap(store2) != PoolConfigStore.unwrap(store3));
        assertEq(store3.totalEntries(), 3);
        (tickSpacing, bundleFee) = store3.get(skey(assets[0], assets[1]), 0);
        assertEq(tickSpacing, 19);
        assertEq(bundleFee, 0.01e6);
        (tickSpacing, bundleFee) = store3.get(skey(assets[3], assets[31]), 1);
        assertEq(tickSpacing, 120);
        assertEq(bundleFee, 0.000134e6);
        (tickSpacing, bundleFee) = store3.get(skey(assets[4], assets[7]), 2);
        assertEq(tickSpacing, 41);
        assertEq(bundleFee, 0.1003e6);
    }

    function test_batchUpdatePools() public {
        vm.startPrank(controller);
        angstrom.configurePool(assets[0], assets[4], 19, 0.01e6, 0.0444e6, 0);
        angstrom.configurePool(assets[2], assets[20], 120, 0.000134e6, 0.01e6, 0);
        angstrom.configurePool(assets[1], assets[8], 41, 0.1003e6, 0.003e6, 0);
        angstrom.configurePool(assets[12], assets[31], 60, 0.00012e6, 0.03e6, 0);
        vm.stopPrank();

        PoolConfigStore store = angstrom.configStore();

        ConfigEntryUpdate[] memory updates = new ConfigEntryUpdate[](2);
        updates[0] = ConfigEntryUpdate(1, skey(assets[2], assets[20]), 0, 0.05e6, 0);
        updates[1] = ConfigEntryUpdate(3, skey(assets[12], assets[31]), 0.001e6, 0.03e6, 0);

        vm.prank(controller);
        angstrom.batchUpdatePools(store, updates);

        (
            StoreKey key,
            uint16 tickSpacing,
            uint24 bundleFee,
            bool unlockedFeeSet,
            uint24 unlockedFee
        ) = getEntry(0);
        assertEq(key, skey(assets[0], assets[4]));
        assertEq(tickSpacing, 19);
        assertEq(bundleFee, 0.01e6);
        assertTrue(unlockedFeeSet);
        assertEq(unlockedFee, 0.0444e6);

        (key, tickSpacing, bundleFee, unlockedFeeSet, unlockedFee) = getEntry(1);
        assertEq(key, skey(assets[2], assets[20]));
        assertEq(tickSpacing, 120);
        assertEq(bundleFee, 0);
        assertTrue(unlockedFeeSet);
        assertEq(unlockedFee, 0.05e6);

        (key, tickSpacing, bundleFee, unlockedFeeSet, unlockedFee) = getEntry(2);
        assertEq(key, skey(assets[1], assets[8]));
        assertEq(tickSpacing, 41);
        assertEq(bundleFee, 0.1003e6);
        assertTrue(unlockedFeeSet);
        assertEq(unlockedFee, 0.003e6);

        (key, tickSpacing, bundleFee, unlockedFeeSet, unlockedFee) = getEntry(3);
        assertEq(key, skey(assets[12], assets[31]));
        assertEq(tickSpacing, 60);
        assertEq(bundleFee, 0.001e6);
        assertTrue(unlockedFeeSet);
        assertEq(unlockedFee, 0.03e6);
    }

    function test_fuzzing_removeExistingWhenGreaterThanOne(uint256 indexToRemove) public {
        indexToRemove = bound(indexToRemove, 0, 2);

        vm.startPrank(controller);
        angstrom.configurePool(assets[0], assets[1], 19, 0.01e6, 0, 0);
        angstrom.configurePool(assets[3], assets[31], 120, 0.000134e6, 0, 0);
        angstrom.configurePool(assets[4], assets[7], 41, 0.1003e6, 0, 0);
        vm.stopPrank();

        PoolConfigStore store = angstrom.configStore();
        vm.prank(controller);
        // forgefmt: disable-next-item
        angstrom.removePool(
            indexToRemove == 0 ? skey(assets[0], assets[1]) : indexToRemove == 1
                ? skey(assets[3], assets[31])
                : skey(assets[4], assets[7]),
            store,
            indexToRemove
        );
        PoolConfigStore storeAfter = angstrom.configStore();
        assertTrue(PoolConfigStore.unwrap(store) != PoolConfigStore.unwrap(storeAfter));
        assertEq(storeAfter.totalEntries(), 2);
        if (indexToRemove == 0) {
            (int24 tickSpacing, uint24 bundleFee) = storeAfter.get(skey(assets[4], assets[7]), 0);
            assertEq(tickSpacing, 41);
            assertEq(bundleFee, 0.1003e6);
            (tickSpacing, bundleFee) = storeAfter.get(skey(assets[3], assets[31]), 1);
            assertEq(tickSpacing, 120);
            assertEq(bundleFee, 0.000134e6);
        } else if (indexToRemove == 1) {
            (int24 tickSpacing, uint24 bundleFee) = storeAfter.get(skey(assets[0], assets[1]), 0);
            assertEq(tickSpacing, 19);
            assertEq(bundleFee, 0.01e6);
            (tickSpacing, bundleFee) = storeAfter.get(skey(assets[4], assets[7]), 1);
            assertEq(tickSpacing, 41);
            assertEq(bundleFee, 0.1003e6);
        } else if (indexToRemove == 2) {
            (int24 tickSpacing, uint24 bundleFee) = storeAfter.get(skey(assets[0], assets[1]), 0);
            assertEq(tickSpacing, 19);
            assertEq(bundleFee, 0.01e6);
            (tickSpacing, bundleFee) = storeAfter.get(skey(assets[3], assets[31]), 1);
            assertEq(tickSpacing, 120);
            assertEq(bundleFee, 0.000134e6);
        }
    }

    function test_fuzzing_removeStandalone(
        address asset0,
        address asset1,
        uint16 tickSpacing,
        uint24 bundleFee
    ) public {
        vm.assume(asset0 != asset1);
        (asset0, asset1) = sort(asset0, asset1);

        tickSpacing = boundTickSpacing(tickSpacing);
        bundleFee = boundE6(bundleFee, MAX_FEE);

        vm.prank(controller);
        angstrom.configurePool(asset0, asset1, tickSpacing, bundleFee, 0, 0);
        PoolConfigStore store = angstrom.configStore();

        vm.prank(controller);
        angstrom.removePool(skey(asset0, asset1), store, 0);

        PoolConfigStore newStore = angstrom.configStore();
        assertEq(newStore.totalEntries(), 0);
        assertEq(PoolConfigStore.unwrap(newStore).code, hex"00");
    }

    function test_configure_existing() public {
        vm.prank(controller);
        angstrom.configurePool(assets[0], assets[1], 190, 0, 0, 0);
        PoolConfigStore store1 = angstrom.configStore();
        assertEq(store1.totalEntries(), 1);
        (int24 tickSpacing, uint24 bundleFee) = store1.get(skey(assets[0], assets[1]), 0);
        assertEq(tickSpacing, 190);
        assertEq(bundleFee, 0);

        vm.prank(controller);
        angstrom.configurePool(assets[0], assets[1], 21, 0.199e6, 0, 0);
        PoolConfigStore store2 = angstrom.configStore();
        assertTrue(PoolConfigStore.unwrap(store1) != PoolConfigStore.unwrap(store2));
        assertEq(store2.totalEntries(), 1);

        (tickSpacing, bundleFee) = store2.get(skey(assets[0], assets[1]), 0);
        assertEq(tickSpacing, 21);
        assertEq(bundleFee, 0.199e6);
    }

    function test_fuzzing_prevents_nonControllerConfiguring(
        address configurer,
        address asset0,
        address asset1,
        uint16 tickSpacing,
        uint24 bundleFee,
        uint24 unlockedFee
    ) public {
        vm.assume(configurer != controller);
        vm.assume(asset0 != asset1);
        tickSpacing = uint16(bound(tickSpacing, 1, type(uint16).max));
        bundleFee = boundE6(bundleFee);
        unlockedFee = boundE6(unlockedFee);
        vm.prank(configurer);
        vm.expectRevert(TopLevelAuth.NotController.selector);
        if (asset0 > asset1) (asset0, asset1) = (asset1, asset0);
        angstrom.configurePool(asset0, asset1, tickSpacing, bundleFee, unlockedFee, 0);
    }

    function test_fuzzing_prevents_nonControllerSettingController(
        address imposterController,
        address newController
    ) public {
        vm.assume(imposterController != controller);
        vm.prank(imposterController);
        vm.expectRevert(TopLevelAuth.NotController.selector);
        angstrom.setController(newController);
    }

    function test_fuzzing_canChangeController(address newController) public {
        assertEq(rawGetController(address(angstrom)), controller);
        vm.prank(controller);
        angstrom.setController(newController);
        assertEq(rawGetController(address(angstrom)), newController);

        if (controller != newController) {
            vm.prank(controller);
            vm.expectRevert(TopLevelAuth.NotController.selector);
            angstrom.setController(controller);
        }
    }

    function test_fuzzing_prevents_providingDuplicate(
        address asset,
        uint16 tickSpacing,
        uint24 bundleFee,
        uint24 unlockedFee
    ) public {
        tickSpacing = boundTickSpacing(tickSpacing);
        bundleFee = boundE6(bundleFee);
        unlockedFee = boundE6(unlockedFee);
        vm.prank(controller);
        vm.expectRevert(TopLevelAuth.AssetsUnordered.selector);
        angstrom.configurePool(asset, asset, tickSpacing, bundleFee, unlockedFee, 0);
    }

    function test_fuzzing_prevents_providingTickSpacingZero(
        address asset0,
        address asset1,
        uint24 bundleFee,
        uint24 unlockedFee
    ) public {
        vm.assume(asset0 != asset1);
        if (asset1 < asset0) (asset0, asset1) = (asset1, asset0);
        bundleFee = boundE6(bundleFee);
        unlockedFee = boundE6(unlockedFee);
        vm.prank(controller);
        vm.expectRevert(ConfigEntryLib.InvalidTickSpacing.selector);
        angstrom.configurePool(asset0, asset1, 0, bundleFee, unlockedFee, 0);
    }

    function test_fuzzing_prevents_settingFeeAboveMax(
        address asset0,
        address asset1,
        uint16 tickSpacing,
        uint24 bundleFee,
        uint24 unlockedFee
    ) public {
        vm.assume(asset0 != asset1);
        if (asset1 < asset0) (asset0, asset1) = (asset1, asset0);
        bundleFee = uint24(bound(bundleFee, MAX_FEE + 1, type(uint24).max));
        unlockedFee = boundE6(unlockedFee);
        tickSpacing = boundTickSpacing(tickSpacing);

        vm.prank(controller);
        vm.expectRevert(ConfigEntryLib.FeeAboveMax.selector);
        angstrom.configurePool(asset0, asset1, tickSpacing, bundleFee, unlockedFee, 0);
    }

    function addrInc(address prev) internal pure returns (address next) {
        assembly ("memory-safe") {
            mstore(0x00, prev)
            let hash := keccak256(0x00, 0x20)
            next := add(prev, shr(120, hash))
        }
    }

    function getEntry(uint256 index)
        internal
        view
        returns (
            StoreKey key,
            uint16 tickSpacing,
            uint24 bundleFee,
            bool unlockedFeeSet,
            uint24 unlockedFee
        )
    {
        PoolConfigStore store = angstrom.configStore();
        ConfigBuffer memory buffer = store.read_to_buffer();
        ConfigEntry entry = buffer.entries[index];

        key = entry.key();
        tickSpacing = entry.tickSpacing();
        bundleFee = entry.bundleFee();
        (unlockedFeeSet, unlockedFee) = AngstromView.unlockedFee(angstrom, key);
    }
}
