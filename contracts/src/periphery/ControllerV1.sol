// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {IAngstromAuth} from "../interfaces/IAngstromAuth.sol";
import {Ownable2Step, Ownable} from "@openzeppelin/contracts/access/Ownable2Step.sol";
import {PoolConfigStoreLib, StoreKey, STORE_HEADER_SIZE} from "../libraries/PoolConfigStore.sol";
import {ConfigEntry, ENTRY_SIZE, KEY_MASK} from "../types/ConfigEntry.sol";
import {EnumerableSet} from "@openzeppelin/contracts/utils/structs/EnumerableSet.sol";

/// @author philogy <https://github.com/philogy>
contract ControllerV1 is Ownable2Step {
    using EnumerableSet for EnumerableSet.AddressSet;

    uint256 constant SCHEDULE_TO_CONFIRM_DELAY = 2 weeks;
    bytes32 constant STORE_ANGSTROM_SLOT = bytes32(uint256(2));
    uint256 constant STORE_RIGHT_SHIFT = 64;

    event NewControllerScheduled(address indexed newController);
    event NewControllerCancelled();
    event NewControllerAccepted();

    event PoolConfigured(
        address indexed asset0, address indexed asset1, uint16 tickSpacing, uint24 feeInE6
    );
    event PoolRemoved(address indexed asset0, address indexed asset1);

    event NodeAdded(address indexed node);
    event NodeRemoved(address indexed node);

    error NotScheduledController();
    error ControllerStillPending();
    error ControllerNotPending();
    error ControllerZero();
    error DuplicateAssets();
    error AlreadyNode();
    error NotNode();

    struct Pool {
        address asset0;
        address asset1;
    }

    IAngstromAuth public immutable ANGSTROM;

    address public pendingController;
    uint40 public scheduledAt;
    EnumerableSet.AddressSet internal _nodes;

    mapping(StoreKey key => Pool) public pools;

    constructor(IAngstromAuth angstrom, address initialOwner) Ownable(initialOwner) {
        ANGSTROM = angstrom;
    }

    function scheduleNewController(address newController) public {
        _checkOwner();
        if (newController == address(0)) revert ControllerZero();
        pendingController = newController;
        scheduledAt = uint40(block.timestamp);
        emit NewControllerScheduled(newController);
    }

    function cancelPendingController() public {
        _checkOwner();
        if (pendingController == address(0)) revert ControllerNotPending();
        pendingController = address(0);
        scheduledAt = 0;
        emit NewControllerCancelled();
    }

    function acceptNewController() public {
        if (block.timestamp < scheduledAt + SCHEDULE_TO_CONFIRM_DELAY) {
            revert ControllerStillPending();
        }
        if (msg.sender != pendingController) revert NotScheduledController();
        pendingController = address(0);
        scheduledAt = 0;
        emit NewControllerAccepted();
        ANGSTROM.setController(msg.sender);
    }

    function configurePool(address asset0, address asset1, uint16 tickSpacing, uint24 feeInE6)
        external
    {
        _checkOwner();
        if (asset0 > asset1) (asset0, asset1) = (asset1, asset0);
        StoreKey key = PoolConfigStoreLib.keyFromAssetsUnchecked(asset0, asset1);
        pools[key] = Pool(asset0, asset1);
        emit PoolConfigured(asset0, asset1, tickSpacing, feeInE6);
        ANGSTROM.configurePool(asset0, asset1, tickSpacing, feeInE6);
    }

    function removePool(address asset0, address asset1) external {
        _checkOwner();
        StoreKey key = PoolConfigStoreLib.keyFromAssetsUnchecked(asset0, asset1);
        address configStore = _configStore();
        uint256 poolIndex = 0;
        while (true) {
            ConfigEntry entry = _getAndCheckStoreEntry(configStore, poolIndex, key);
            if (!entry.isEmpty()) break;
            poolIndex++;
        }
        emit PoolRemoved(asset0, asset1);
        ANGSTROM.removePool(configStore, poolIndex);
    }

    function addNode(address node) external {
        _checkOwner();
        if (!_nodes.add(node)) revert AlreadyNode();
        emit NodeAdded(node);
        _toggle(node);
    }

    function removeNode(address node) external {
        _checkOwner();
        if (!_nodes.remove(node)) revert NotNode();
        emit NodeRemoved(node);
        _toggle(node);
    }

    function totalNodes() public view returns (uint256) {
        return _nodes.length();
    }

    /// @dev Loads all node addresses from storage, could exceed gas limit if a lot of nodes are
    /// added.
    function nodes() public view returns (address[] memory) {
        return _nodes.values();
    }

    function _configStore() internal view returns (address addr) {
        bytes32 slotValue = ANGSTROM.extsload(STORE_ANGSTROM_SLOT);
        assembly ("memory-safe") {
            addr := shr(STORE_RIGHT_SHIFT, slotValue)
        }
    }

    function _getAndCheckStoreEntry(address configStore, uint256 index, StoreKey key)
        internal
        view
        returns (ConfigEntry entry)
    {
        uint256 offset = STORE_HEADER_SIZE + index * ENTRY_SIZE;
        assembly ("memory-safe") {
            extcodecopy(configStore, 0x00, offset, ENTRY_SIZE)
            entry := mload(0x00)
            entry := mul(entry, eq(key, and(entry, KEY_MASK)))
        }
    }

    function _toggle(address node) internal {
        address[] memory nodesToToggle = new address[](1);
        nodesToToggle[0] = node;
        ANGSTROM.toggleNodes(nodesToToggle);
    }
}
