// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "test/_helpers/BaseTest.sol";
import {Angstrom} from "src/Angstrom.sol";
import {PoolManager} from "v4-core/src/PoolManager.sol";
import {IControllerV1} from "test/_helpers/IControllerV1.sol";
import {TopLevelAuth} from "src/modules/TopLevelAuth.sol";
import {LibSort} from "solady/src/utils/LibSort.sol";
import {
    PoolConfigStoreLib, PoolConfigStore, StoreKey
} from "../../src/libraries/PoolConfigStore.sol";

import {console} from "forge-std/console.sol";

/// @author philogy <https://github.com/philogy>
contract ControllerV1Test is BaseTest {
    Angstrom angstrom;
    PoolManager uni;
    IControllerV1 controller;

    address pm_owner = makeAddr("pm_owner");
    address temp_controller = makeAddr("temp_controller");
    address controller_owner = makeAddr("controller_owner");

    function setUp() public {
        uni = new PoolManager(pm_owner);
        angstrom = Angstrom(deployAngstrom(type(Angstrom).creationCode, uni, temp_controller));
        controller =
            IControllerV1(deployCode("ControllerV1", abi.encode(angstrom, controller_owner)));
        vm.prank(temp_controller);
        angstrom.setController(address(controller));
    }

    function test_fuzzing_initializesOwner(address startingOwner) public {
        IControllerV1 c =
            IControllerV1(deployCode("ControllerV1", abi.encode(angstrom, startingOwner)));
        assertEq(c.owner(), startingOwner);
    }

    function test_fuzzing_canTransferOwner(address newOwner) public {
        vm.assume(newOwner != address(0));

        vm.prank(controller_owner);
        controller.transfer_ownership(newOwner);
        vm.prank(newOwner);
        controller.accept_ownership();

        assertEq(controller.owner(), newOwner);

        if (newOwner != controller_owner) {
            vm.prank(controller_owner);
            vm.expectRevert("ownable: caller is not the owner");
            controller.transfer_ownership(controller_owner);
        }
    }

    function test_can_cancelNewController() public {
        address bad_controller = makeAddr("bad_controller");
        uint256 unlockTime = block.timestamp + controller.SCHEDULE_TO_CONFIRM_DELAY();
        vm.expectEmit(true, true, true, true);
        emit IControllerV1.NewControllerScheduled(bad_controller, unlockTime);
        vm.prank(controller_owner);
        controller.schedule_new_controller(bad_controller);

        skip(1 days);

        vm.expectEmit(true, true, true, true);
        emit IControllerV1.NewControllerCancelled();
        vm.prank(controller_owner);
        controller.cancel_pending_controller();

        vm.warp(unlockTime);
        vm.expectRevert("Controller not pending");
        controller.confirm_pending_controller();
    }

    function test_configurePools() public {
        address[] memory assets =
            addrs(abi.encode(makeAddr("asset_1"), makeAddr("asset_2"), makeAddr("asset_3")));
        LibSort.sort(assets);

        vm.expectEmit(true, true, true, true);
        emit IControllerV1.PoolConfigured(assets[0], assets[2], 100, 0);
        vm.breakpoint("c");
        vm.prank(controller_owner);
        controller.configure_pool(assets[0], assets[2], 100, 0);

        PoolConfigStore store = PoolConfigStore.wrap(rawGetConfigStore(address(angstrom)));
        StoreKey key = skey(assets[0], assets[2]);
        (int24 tickSpacing, uint24 feeInE6) = store.get(key, 0);
        assertEq(tickSpacing, 100);
        assertEq(feeInE6, 0);
        (address asset0, address asset1) = controller.pools(StoreKey.unwrap(key));
        assertEq(asset0, assets[0]);
        assertEq(asset1, assets[2]);
    }

    function test_fuzzing_preventsNonOwnerTransfer(address nonOwner, address newOwner) public {
        vm.assume(nonOwner != controller_owner);
        vm.prank(nonOwner);
        vm.expectRevert("ownable: caller is not the owner");
        controller.transfer_ownership(newOwner);
    }

    function test_controllerMigration() public {
        address next_controller = makeAddr("next_controller");
        vm.expectEmit(true, true, true, true);
        emit IControllerV1.NewControllerScheduled(
            next_controller, block.timestamp + controller.SCHEDULE_TO_CONFIRM_DELAY()
        );
        vm.prank(controller_owner);
        controller.schedule_new_controller(next_controller);
        assertEq(controller.pending_controller(), next_controller);

        skip(2 days);

        vm.expectRevert("New controller still pending");
        controller.confirm_pending_controller();

        skip(controller.SCHEDULE_TO_CONFIRM_DELAY());

        vm.expectEmit(true, true, true, true);
        emit IControllerV1.NewControllerConfirmed(next_controller);
        controller.confirm_pending_controller();

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
        assertEq(controller.total_nodes(), 0);
        for (uint256 i = 0; i < addrs.length; i++) {
            vm.expectEmit(true, true, true, true);
            emit IControllerV1.NodeAdded(addrs[i]);
            vm.prank(controller_owner);
            controller.add_node(addrs[i]);
            assertTrue(_isNode(addrs[i]), "expected to be node after add");
            assertEq(controller.total_nodes(), i + 1);
            for (uint256 j = 0; j < i; j++) {
                uint256 totalNodes = controller.total_nodes();
                bool found = false;
                for (uint256 k = 0; k < totalNodes; k++) {
                    if (controller.nodes(k) == addrs[j]) {
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
            emit IControllerV1.NodeRemoved(addrs[ai]);
            vm.prank(controller_owner);
            controller.remove_node(addrs[ai]);
            removed[ai] = true;
            assertEq(controller.total_nodes(), _TOTAL_NODES - i - 1);

            for (uint256 j = 0; j < addrs.length; j++) {
                uint256 totalNodes = controller.total_nodes();
                bool found = false;
                for (uint256 k = 0; k < totalNodes; k++) {
                    if (controller.nodes(k) == addrs[j]) {
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

    function _allNodes() internal view returns (string memory) {
        if (controller.total_nodes() == 0) return "[]";

        string memory s = string.concat("[", vm.toString(controller.nodes(0))); // ]
        for (uint256 i = 1; i < controller.total_nodes(); i++) {}

        return string.concat(s, "]");
    }
}
