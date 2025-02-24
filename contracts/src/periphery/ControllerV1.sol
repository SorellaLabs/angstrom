// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {IAngstromAuth} from "../interfaces/IAngstromAuth.sol";
import {Ownable2Step, Ownable} from "@openzeppelin/contracts/access/Ownable2Step.sol";
import {
    PoolConfigStore,
    PoolConfigStoreLib,
    StoreKey,
    STORE_HEADER_SIZE
} from "../libraries/PoolConfigStore.sol";
import {ConfigEntry, ENTRY_SIZE, KEY_MASK} from "../types/ConfigEntry.sol";
import {EnumerableSet} from "@openzeppelin/contracts/utils/structs/EnumerableSet.sol";
import {SafeTransferLib} from "solady/src/utils/SafeTransferLib.sol";
import {AngstromView} from "./AngstromView.sol";

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
    using AngstromView for IAngstromAuth;
    using EnumerableSet for EnumerableSet.AddressSet;
    using SafeTransferLib for address;

    event NewControllerSet(address indexed newController);
    event NewControllerAccepted(address indexed newController);

    event PoolConfigured(
        address indexed asset0,
        address indexed asset1,
        uint16 tickSpacing,
        uint24 bundleFee,
        uint24 unlockedFee
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
    error NonexistentPool(address asset0, address asset1);
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

    function configurePool(
        address asset0,
        address asset1,
        uint16 tickSpacing,
        uint24 bundleFee,
        uint24 unlockedFee
    ) external {
        _checkOwner();
        if (asset0 > asset1) (asset0, asset1) = (asset1, asset0);
        StoreKey key = PoolConfigStoreLib.keyFromAssetsUnchecked(asset0, asset1);
        pools[key] = Pool(asset0, asset1);

        emit PoolConfigured(asset0, asset1, tickSpacing, bundleFee, unlockedFee);
        ANGSTROM.configurePool(asset0, asset1, tickSpacing, bundleFee, unlockedFee);
    }

    function removePool(address asset0, address asset1) external {
        _checkOwner();

        if (asset0 > asset1) (asset0, asset1) = (asset1, asset0);
        StoreKey key = PoolConfigStoreLib.keyFromAssetsUnchecked(asset0, asset1);
        PoolConfigStore configStore = ANGSTROM.configStore();
        uint256 poolIndex = 0;

        uint256 totalEntries = configStore.totalEntries();
        ConfigEntry entry;
        while (true) {
            entry = configStore.getWithDefaultEmpty(key, poolIndex);
            if (!entry.isEmpty()) break;
            poolIndex++;
            if (poolIndex >= totalEntries) {
                revert NonexistentPool(asset0, asset1);
            }
        }

        emit PoolRemoved(asset0, asset1, entry.tickSpacing(), entry.bundleFee());
        ANGSTROM.removePool(key, configStore, poolIndex);
    }

    function distributeFees(Asset[] calldata assets) external {
        _checkOwner();

        uint256 totalAssets = assets.length;
        for (uint256 i = 0; i < totalAssets; i++) {
            Asset calldata asset = assets[i];
            uint256 totalRemaining = asset.total;
            ANGSTROM.pullFee(asset.addr, totalRemaining);
            for (uint256 j = 0; j < asset.dists.length; j++) {
                Distribution calldata dist = asset.dists[j];
                asset.addr.safeTransfer(dist.to, dist.amount);
                totalRemaining -= dist.amount;
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

    function _toggle(address node) internal {
        address[] memory nodesToToggle = new address[](1);
        nodesToToggle[0] = node;
        ANGSTROM.toggleNodes(nodesToToggle);
    }
}
