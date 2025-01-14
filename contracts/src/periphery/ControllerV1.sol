// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {IAngstromAuth} from "../interfaces/IAngstromAuth.sol";
import {Ownable2Step, Ownable} from "@openzeppelin/contracts/access/Ownable2Step.sol";
import {PoolConfigStoreLib, StoreKey, STORE_HEADER_SIZE} from "../libraries/PoolConfigStore.sol";
import {ConfigEntry, ENTRY_SIZE, KEY_MASK} from "../types/ConfigEntry.sol";
import {EnumerableSet} from "@openzeppelin/contracts/utils/structs/EnumerableSet.sol";
import {SafeTransferLib} from "solady/src/utils/SafeTransferLib.sol";

struct Distribution {
    address to;
    uint256 amount;
}

struct Asset {
    address addr;
    uint256 total;
    Distribution[] dists;
}

/// @author philogy <https://github.com/philogy>
contract ControllerV1 is Ownable2Step {
    using EnumerableSet for EnumerableSet.AddressSet;
    using SafeTransferLib for address;

    bytes32 constant STORE_ANGSTROM_SLOT = bytes32(uint256(2));
    uint256 constant STORE_RIGHT_SHIFT = 64;

    event NewControllerSet(address indexed newController);
    event NewControllerAccepted(address indexed newController);

    event PoolConfigured(
        address indexed asset0, address indexed asset1, uint16 tickSpacing, uint24 feeInE6
    );

    event PoolRemoved(
        address indexed asset0, address indexed asset1, int24 tickSpacing, uint24 feeInE6
    );

    event NodeAdded(address indexed node);
    event NodeRemoved(address indexed node);

    error NotSetController();
    error DuplicateAssets();
    error AlreadyNode();
    error NotNode();
    error TotalNotDistributed();

    struct Pool {
        address asset0;
        address asset1;
    }

    IAngstromAuth public immutable ANGSTROM;

    address public setController;
    EnumerableSet.AddressSet internal _nodes;

    mapping(StoreKey key => Pool) public pools;

    constructor(IAngstromAuth angstrom, address initialOwner) Ownable(initialOwner) {
        ANGSTROM = angstrom;
    }

    function getAllPools(StoreKey[] calldata storeKeys) external view returns (Pool[] memory) {
        Pool[] memory allPools = new Pool[](storeKeys.length);

        for (uint256 i = 0; i < storeKeys.length; i++) {
            allPools[i] = pools[storeKeys[i]];
        }

        return allPools;
    }

    function addPoolToMap(address asset0, address asset1) external {
        _checkOwner();
        if (asset0 > asset1) (asset0, asset1) = (asset1, asset0);
        StoreKey key = PoolConfigStoreLib.keyFromAssetsUnchecked(asset0, asset1);
        pools[key] = Pool(asset0, asset1);
    }

    function setNewController(address newController) public {
        _checkOwner();
        setController = newController;
        emit NewControllerSet(newController);
    }

    function acceptNewController() public {
        if (msg.sender != setController) revert NotSetController();
        setController = address(0);
        emit NewControllerAccepted(msg.sender);
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

        if (asset0 > asset1) (asset0, asset1) = (asset1, asset0);
        StoreKey key = PoolConfigStoreLib.keyFromAssetsUnchecked(asset0, asset1);
        address configStore = _configStore();
        uint256 poolIndex = 0;

        int24 tickSpacing;
        uint24 feeInE6;

        while (true) {
            ConfigEntry entry = _getAndCheckStoreEntry(configStore, poolIndex, key);
            if (entry.matchingStoreKey(asset0, asset1)) {
                tickSpacing = entry.tickSpacing();
                feeInE6 = entry.feeInE6();
            }

            if (!entry.isEmpty()) break;

            poolIndex++;
        }
        emit PoolRemoved(asset0, asset1, tickSpacing, feeInE6);
        ANGSTROM.removePool(configStore, poolIndex);
    }

    function distributeFees(Asset[] calldata assets) external {
        _checkOwner();

        uint256 totalAssets = assets.length;
        for (uint256 i = 0; i < totalAssets; i++) {
            Asset calldata asset = assets[i];
            address addr = asset.addr;
            uint256 totalRemaining = asset.total;
            ANGSTROM.pullFee(addr, totalRemaining);
            uint256 totalDists = asset.dists.length;
            for (uint256 j = 0; j < totalDists; j++) {
                Distribution calldata dist = asset.dists[j];
                uint256 amount = dist.amount;
                addr.safeTransfer(dist.to, amount);
                totalRemaining -= amount;
            }
            if (totalRemaining != 0) revert TotalNotDistributed();
        }
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

    /// @dev Loads all node addresses from storage, could exceed gas limit if too many nodes are added.
    function nodes() public view returns (address[] memory) {
        return _nodes.values();
    }

    function _configStore() public view returns (address addr) {
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
