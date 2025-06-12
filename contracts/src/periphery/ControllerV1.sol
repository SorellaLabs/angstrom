// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {IAngstromAuth, ConfigEntryUpdate} from "../interfaces/IAngstromAuth.sol";
import {Ownable} from "solady/src/auth/Ownable.sol";
import {PoolConfigStore, STORE_HEADER_SIZE} from "../libraries/PoolConfigStore.sol";
import {StoreKey, StoreKeyLib} from "../types/StoreKey.sol";
import {ConfigEntry, ENTRY_SIZE, KEY_MASK} from "../types/ConfigEntry.sol";
import {AddressSet} from "solady/src/utils/g/EnumerableSetLib.sol";
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
/// @author Will Smith <https://github.com/Will-Smith11>
contract ControllerV1 is Ownable {
    using AngstromView for IAngstromAuth;
    using SafeTransferLib for address;

    /// @dev Controller enforced fee maximum (can be overriden by swapping out controller).
    uint256 internal constant MAX_FEE_BPS = 0.01e6;

    event NewControllerSet(address indexed newController);
    event NewControllerAccepted(address indexed newController);

    event PoolConfigured(
        address indexed asset0,
        address indexed asset1,
        uint16 tickSpacing,
        uint24 bundleFee,
        uint24 unlockedFee,
        uint24 protocolUnlockedFee
    );

    event OpaqueBatchPoolUpdate();

    event PoolRemoved(
        address indexed asset0, address indexed asset1, int24 tickSpacing, uint24 feeInE6
    );

    event NodeAdded(address indexed node);
    event NodeRemoved(address indexed node);

    error FeeAboveMax();
    error NotSetController();
    error AlreadyNode();
    error NotNodeOrOwner();
    error NotNode();
    error NonexistentPool(address asset0, address asset1);
    error KeyNotFound();
    error TotalNotDistributed();
    error FunctionDisabled();

    struct Pool {
        address asset0;
        address asset1;
    }

    IAngstromAuth public immutable ANGSTROM;

    address public setController;
    AddressSet internal _nodes;

    Pool[] internal _pools;
    mapping(StoreKey key => uint256 maybeIndex) internal _poolIndices;

    address public immutable fastOwner;
    bool internal hasSetInit;

    constructor(IAngstromAuth angstrom, address initialOwner, address _fastOwner) {
        _initializeOwner(initialOwner);
        ANGSTROM = angstrom;
        fastOwner = _fastOwner;
    }

    function initStartNodes(address[] memory initNodes) public {
        if (hasSetInit) return;
        hasSetInit = true;

        for (uint256 i = 0; i < initNodes.length; i++) {
            address node = initNodes[i];
            if (!_nodes.add(node)) revert AlreadyNode();
            _toggle(node);
            emit NodeAdded(node);
        }
    }

    function transferOwnership(address) public payable override {
        revert FunctionDisabled();
    }

    function renounceOwnership() public payable override {
        revert FunctionDisabled();
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

    function collect_unlock_swap_fees(address to, bytes calldata packed_assets) external {
        _checkFastOwner();

        ANGSTROM.collect_unlock_swap_fees(to, packed_assets);
    }

    function configurePool(
        address asset0,
        address asset1,
        uint16 tickSpacing,
        uint24 bundleFee,
        uint24 unlockedFee,
        uint24 protocolUnlockedFee
    ) external {
        _checkFastOwner();

        if (bundleFee > MAX_FEE_BPS) revert FeeAboveMax();
        if (unlockedFee > MAX_FEE_BPS) revert FeeAboveMax();

        // Call to `.configurePool` will check for us whether `asset0 == asset1`.
        if (asset0 > asset1) (asset0, asset1) = (asset1, asset0);
        StoreKey key = StoreKeyLib.keyFromAssetsUnchecked(asset0, asset1);

        uint256 maybe_index = _poolIndices[key];
        if (maybe_index == 0) {
            _pools.push(Pool(asset0, asset1));
            maybe_index = _pools.length;
            _poolIndices[key] = maybe_index;
        }

        emit PoolConfigured(
            asset0, asset1, tickSpacing, bundleFee, unlockedFee, protocolUnlockedFee
        );
        ANGSTROM.configurePool(
            asset0, asset1, tickSpacing, bundleFee, unlockedFee, protocolUnlockedFee
        );
    }

    function removePool(address asset0, address asset1) external {
        _checkOwner();

        if (asset0 > asset1) (asset0, asset1) = (asset1, asset0);
        StoreKey key = StoreKeyLib.keyFromAssetsUnchecked(asset0, asset1);

        unchecked {
            uint256 index_plus_one = _poolIndices[key];
            if (index_plus_one == 0) revert NonexistentPool(asset0, asset1);
            uint256 index = index_plus_one - 1;

            uint256 length = _pools.length;
            if (index_plus_one < length) {
                Pool memory last_pool = _pools[length - 1];
                StoreKey last_key =
                    StoreKeyLib.keyFromAssetsUnchecked(last_pool.asset0, last_pool.asset1);
                _pools[index] = last_pool;
                _poolIndices[last_key] = index_plus_one;
            }

            _poolIndices[key] = 0;
            _pools.pop();

            PoolConfigStore config_store = ANGSTROM.configStore();
            (int24 tick_spacing, uint24 bundle_fee) = config_store.get(key, index);
            emit PoolRemoved(asset0, asset1, tick_spacing, bundle_fee);
            ANGSTROM.removePool(key, config_store, index);
        }
    }

    struct PoolUpdate {
        address assetA;
        address assetB;
        uint24 bundleFee;
        uint24 unlockedFee;
        uint24 protocolUnlockedFee;
    }

    function batchUpdatePools(PoolUpdate[] calldata updates) external {
        _checkNodeOrFastOwner();

        ConfigEntryUpdate[] memory entry_updates = new ConfigEntryUpdate[](updates.length);

        for (uint256 i = 0; i < updates.length; i++) {
            PoolUpdate calldata update = updates[i];
            if (update.bundleFee > MAX_FEE_BPS) revert FeeAboveMax();
            if (update.unlockedFee > MAX_FEE_BPS) revert FeeAboveMax();

            (address asset0, address asset1) = (update.assetA, update.assetB);
            if (asset1 > asset0) (asset0, asset1) = (asset1, asset0);
            StoreKey key = StoreKeyLib.keyFromAssetsUnchecked(asset0, asset1);

            entry_updates[i] = ConfigEntryUpdate({
                index: _poolIndices[key] - 1,
                key: key,
                bundleFee: update.bundleFee,
                unlockedFee: update.unlockedFee,
                protocolUnlockedFee: update.protocolUnlockedFee
            });
        }

        emit OpaqueBatchPoolUpdate();

        PoolConfigStore config_store = ANGSTROM.configStore();
        ANGSTROM.batchUpdatePools(config_store, entry_updates);
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
        _checkFastOwner();
        if (!_nodes.remove(node)) revert NotNode();
        emit NodeRemoved(node);
        _toggle(node);
    }

    function totalNodes() public view returns (uint256) {
        return _nodes.length();
    }

    function totalPools() public view returns (uint256) {
        return _pools.length;
    }

    function getPoolByIndex(uint256 index) public view returns (address asset0, address asset1) {
        Pool storage pool = _pools[index];
        asset0 = pool.asset0;
        asset1 = pool.asset1;
    }

    function getPoolByKey(StoreKey key) public view returns (address asset0, address asset1) {
        uint256 index_plus_one = _poolIndices[key];
        if (index_plus_one == 0) revert KeyNotFound();
        Pool storage pool = _pools[index_plus_one - 1];
        asset0 = pool.asset0;
        asset1 = pool.asset1;
    }

    function keyExists(StoreKey key) public view returns (bool) {
        return _poolIndices[key] > 0;
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

    function _checkNodeOrFastOwner() internal view {
        if (msg.sender == fastOwner) return;
        if (_nodes.contains(msg.sender)) return;
        if (msg.sender == owner()) return;
        revert NotNodeOrOwner();
    }

    function _checkFastOwner() internal view {
        if (msg.sender == fastOwner) return;
        if (msg.sender == owner()) return;
        revert NotNodeOrOwner();
    }

    function _checkNodeOrOwner() internal view {
        if (_nodes.contains(msg.sender)) return;
        if (owner() == msg.sender) return;
        revert NotNodeOrOwner();
    }
}
