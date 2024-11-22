// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "test/_helpers/BaseTest.sol";
import {Angstrom} from "src/Angstrom.sol";
import {PoolManager} from "v4-core/src/PoolManager.sol";
import {ControllerV1} from "src/periphery/ControllerV1.sol";
import {TopLevelAuth} from "src/modules/TopLevelAuth.sol";
import {LibSort} from "solady/src/utils/LibSort.sol";
import {
    PoolConfigStoreLib, PoolConfigStore, StoreKey
} from "../../src/libraries/PoolConfigStore.sol";
import {Ownable2Step, Ownable} from "@openzeppelin/contracts/access/Ownable2Step.sol";

import {console} from "forge-std/console.sol";

/// @author philogy <https://github.com/philogy>
contract ControllerV1Test is BaseTest {
    uint256 constant SCHEDULE_TO_CONFIRM_DELAY = 2 weeks;

    Angstrom angstrom;
    PoolManager uni;
    ControllerV1 controller;

    address pm_owner = makeAddr("pm_owner");
    address temp_controller = makeAddr("temp_controller");
    address controller_owner = makeAddr("controller_owner");

    function setUp() public {
        uni = new PoolManager(pm_owner);
        angstrom = Angstrom(deployAngstrom(type(Angstrom).creationCode, uni, temp_controller));
        controller = new ControllerV1(angstrom, controller_owner);
        vm.prank(temp_controller);
        angstrom.setController(address(controller));
    }

    function test_fuzzing_initializesOwner(address startingOwner) public {
        vm.assume(startingOwner != address(0));
        ControllerV1 c = new ControllerV1(angstrom, startingOwner);
        assertEq(c.owner(), startingOwner);
    }

    function test_fuzzing_canTransferOwner(address newOwner) public {
        vm.assume(newOwner != address(0));

        vm.prank(controller_owner);
        controller.transferOwnership(newOwner);
        vm.prank(newOwner);
        controller.acceptOwnership();

        assertEq(controller.owner(), newOwner);

        if (newOwner != controller_owner) {
            vm.prank(controller_owner);
            vm.expectRevert(
                abi.encodeWithSelector(
                    Ownable.OwnableUnauthorizedAccount.selector, controller_owner
                )
            );
            controller.transferOwnership(controller_owner);
        }
    }

    function test_can_cancelNewController() public {
        address bad_controller = makeAddr("bad_controller");
        uint256 unlockTime = block.timestamp + SCHEDULE_TO_CONFIRM_DELAY;
        vm.expectEmit(true, true, true, true);
        emit ControllerV1.NewControllerScheduled(bad_controller);
        vm.prank(controller_owner);
        controller.scheduleNewController(bad_controller);

        skip(1 days);

        vm.expectEmit(true, true, true, true);
        emit ControllerV1.NewControllerCancelled();
        vm.prank(controller_owner);
        controller.cancelPendingController();

        vm.warp(unlockTime);
        vm.expectRevert(ControllerV1.ControllerNotPending.selector);
        vm.prank(controller_owner);
        controller.cancelPendingController();
    }

    function test_configurePools() public {
        address[] memory assets =
            addrs(abi.encode(makeAddr("asset_1"), makeAddr("asset_2"), makeAddr("asset_3")));
        LibSort.sort(assets);

        vm.expectEmit(true, true, true, true);
        emit ControllerV1.PoolConfigured(assets[0], assets[2], 100, 0);
        vm.prank(controller_owner);
        controller.configurePool(assets[0], assets[2], 100, 0);

        PoolConfigStore store = PoolConfigStore.wrap(rawGetConfigStore(address(angstrom)));
        assertEq(store.totalEntries(), 1);
        StoreKey key = skey(assets[0], assets[2]);
        (int24 tickSpacing, uint24 feeInE6) = store.get(key, 0);
        assertEq(tickSpacing, 100);
        assertEq(feeInE6, 0);
        (address asset0, address asset1) = controller.pools(key);
        assertEq(asset0, assets[0]);
        assertEq(asset1, assets[2]);

        vm.expectEmit(true, true, true, true);
        emit ControllerV1.PoolRemoved(assets[0], assets[2]);
        vm.prank(controller_owner);
        controller.removePool(assets[0], assets[2]);
    }

    function test_fuzzing_preventsNonOwnerTransfer(address nonOwner, address newOwner) public {
        vm.assume(nonOwner != controller_owner);
        vm.prank(nonOwner);
        vm.expectRevert(
            abi.encodeWithSelector(Ownable.OwnableUnauthorizedAccount.selector, nonOwner)
        );
        controller.transferOwnership(newOwner);
    }

    function test_controllerMigration() public {
        address next_controller = makeAddr("next_controller");
        vm.expectEmit(true, true, true, true);
        emit ControllerV1.NewControllerScheduled(next_controller);
        vm.prank(controller_owner);
        controller.scheduleNewController(next_controller);
        assertEq(controller.pendingController(), next_controller);

        skip(2 days);

        vm.expectRevert(ControllerV1.ControllerStillPending.selector);
        vm.prank(next_controller);
        controller.acceptNewController();

        skip(SCHEDULE_TO_CONFIRM_DELAY);

        vm.expectEmit(true, true, true, true);
        emit ControllerV1.NewControllerAccepted();
        vm.prank(next_controller);
        controller.acceptNewController();

        assertEq(rawGetController(address(angstrom)), next_controller);
    }

    uint256 constant _TOTAL_NODES = 5;

    function test_addRemoveNode() public {
        address[_TOTAL_NODES] memory addrs = [
            makeAddr("addr_1"),
            makeAddr("addr_2"),
            makeAddr("addr_3"),
            makeAddr("addr_4"),
            makeAddr("addr_5")
        ];
        assertEq(controller.totalNodes(), 0);
        for (uint256 i = 0; i < addrs.length; i++) {
            vm.expectEmit(true, true, true, true);
            emit ControllerV1.NodeAdded(addrs[i]);
            vm.prank(controller_owner);
            controller.addNode(addrs[i]);
            assertTrue(_isNode(addrs[i]), "expected to be node after add");
            assertEq(controller.totalNodes(), i + 1);
            for (uint256 j = 0; j < i; j++) {
                uint256 totalNodes = controller.totalNodes();
                bool found = false;
                for (uint256 k = 0; k < totalNodes; k++) {
                    if (controller.nodes()[k] == addrs[j]) {
                        found = true;
                        break;
                    }
                }
                assertTrue(found, "Not in node list while adding");
            }
        }

        uint256[_TOTAL_NODES] memory removeMap = [uint256(2), 4, 0, 3, 1];
        bool[_TOTAL_NODES] memory removed;
        for (uint256 i = 0; i < removeMap.length; i++) {
            uint256 ai = removeMap[i];

            vm.expectEmit(true, true, true, true);
            emit ControllerV1.NodeRemoved(addrs[ai]);
            vm.prank(controller_owner);
            controller.removeNode(addrs[ai]);
            removed[ai] = true;
            assertEq(controller.totalNodes(), _TOTAL_NODES - i - 1);

            for (uint256 j = 0; j < addrs.length; j++) {
                uint256 totalNodes = controller.totalNodes();
                bool found = false;
                for (uint256 k = 0; k < totalNodes; k++) {
                    if (controller.nodes()[k] == addrs[j]) {
                        found = true;
                        break;
                    }
                }
                if (removed[j]) {
                    assertFalse(found, "Found when didn't expect to");
                    assertFalse(_isNode(addrs[j]), "expected not node after removal");
                } else {
                    assertTrue(found, "Not found when expected");
                    assertTrue(_isNode(addrs[j]), "expected node before removal");
                }
            }
        }
    }

    function _isNode(address node) internal returns (bool) {
        bumpBlock();
        vm.prank(node);
        try angstrom.execute(new bytes(15)) {
            return true;
        } catch (bytes memory error) {
            require(keccak256(error) == keccak256(abi.encodePacked(TopLevelAuth.NotNode.selector)));
            return false;
        }
    }
}
