// SPDX-License-Identifier: MIT
pragma solidity ^0.8.26;

import {IAngstromAuth, ConfigEntryUpdate} from "../interfaces/IAngstromAuth.sol";
import {UniConsumer} from "./UniConsumer.sol";
import {EIP712} from "solady/src/utils/EIP712.sol";

import {PoolConfigStore, PoolConfigStoreLib} from "../libraries/PoolConfigStore.sol";
import {StoreKey, StoreKeyLib} from "../types/StoreKey.sol";
import {ConfigEntry, ConfigEntryLib} from "../types/ConfigEntry.sol";
import {ConfigBuffer} from "../types/ConfigBuffer.sol";
import {IHooks} from "v4-core/src/interfaces/IHooks.sol";
import {PoolKey} from "v4-core/src/types/PoolKey.sol";
import {SafeCastLib} from "solady/src/utils/SafeCastLib.sol";
import {LPFeeLibrary} from "v4-core/src/libraries/LPFeeLibrary.sol";
import {SafeTransferLib} from "solady/src/utils/SafeTransferLib.sol";
import {SignatureCheckerLib} from "solady/src/utils/SignatureCheckerLib.sol";
import {UnlockSwapFeeCollector} from "./UnlockSwapFeeCollector.sol";

/// @dev Maximum fee that the `bundleFee` for any given pool should be settable to.
uint256 constant MAX_UNLOCK_FEE_E6 = 0.4e6;

/// @author philogy <https://github.com/philogy>
/// @author Will Smith <https://github.com/Will-Smith11>
abstract contract TopLevelAuth is EIP712, UniConsumer, IAngstromAuth {
    using LPFeeLibrary for uint24;
    using SafeTransferLib for address;

    error AssetsUnordered();
    error NotController();
    error UnlockedFeeNotSet(StoreKey key);
    error OnlyOncePerBlock();
    error NotNode();
    error IndexMayHaveChanged();
    error InvalidSignature();
    error UnlockFeeAboveMax();

    /// @dev `keccak256("AttestAngstromBlockEmpty(uint64 block_number)")`
    uint256 internal constant ATTEST_EMPTY_BLOCK_TYPE_HASH =
        0x3f25e551746414ff93f076a7dd83828ff53735b39366c74015637e004fcb0223;

    UnlockSwapFeeCollector internal immutable FEE_COLLECTOR;

    /// @dev Contract that manages all special privileges for contract (setting new nodes,
    /// configuring pools, pulling fees).
    address internal _controller;

    mapping(address => bool) internal _isNode;

    struct UnlockedFees {
        uint24 unlockedFee;
        uint24 protocolUnlockedFee;
    }

    /// @dev Stores the unlocked fees.
    mapping(StoreKey => UnlockedFees) internal _unlockedFees;

    uint64 internal _lastBlockUpdated;
    PoolConfigStore internal _configStore;

    constructor(address controller) {
        _controller = controller;
        FEE_COLLECTOR = new UnlockSwapFeeCollector(UNI_V4);
    }

    function setController(address newController) public override {
        _onlyController();
        _controller = newController;
    }

    function collect_unlock_swap_fees(address to, bytes calldata packed_assets) external override {
        _onlyController();
        FEE_COLLECTOR.withdraw_to(to, packed_assets);
    }

    /// @dev Configure an existing pool or allow the creation of a new pool. Permissioned, only
    /// controller should be allowed to configure.
    function configurePool(
        address asset0,
        address asset1,
        uint16 tickSpacing,
        uint24 bundleFee,
        uint24 unlockedFee,
        uint24 protocolUnlockedFee
    ) external override {
        _onlyController();

        if (asset0 >= asset1) revert AssetsUnordered();

        StoreKey key = StoreKeyLib.keyFromAssetsUnchecked(asset0, asset1);

        ConfigBuffer memory buffer = _configStore.read_to_buffer(1);
        uint256 i = 0;
        uint256 entry_count = buffer.entries.length;

        // Search existing entries and modify the respective entry if found.
        for (; i < entry_count; i++) {
            ConfigEntry entry = buffer.entries[i];
            if (entry.key() == key) {
                buffer.entries[i] =
                    buffer.entries[i].setTickSpacing(tickSpacing).setBundleFee(bundleFee);
                break;
            }
        }
        // If not found push new entry.
        if (i == entry_count) {
            // Safety: Know that `key` is unique because every other key was checked.
            buffer.unsafe_add(ConfigEntryLib.init(key, tickSpacing, bundleFee));
        }

        _configStore = PoolConfigStoreLib.store_from_buffer(buffer);

        _unlockedFees[key] =
            UnlockedFees({unlockedFee: unlockedFee, protocolUnlockedFee: protocolUnlockedFee});

        protocolUnlockedFee.validate();
        unlockedFee.validate();
    }

    function initializePool(
        address assetA,
        address assetB,
        uint256 storeIndex,
        uint160 sqrtPriceX96
    ) public {
        if (assetA > assetB) (assetA, assetB) = (assetB, assetA);
        StoreKey key = StoreKeyLib.keyFromAssetsUnchecked(assetA, assetB);
        (int24 tickSpacing,) = _configStore.get(key, storeIndex);
        UNI_V4.initialize(
            PoolKey(_c(assetA), _c(assetB), INIT_HOOK_FEE, tickSpacing, IHooks(address(this))),
            sqrtPriceX96
        );
    }

    function removePool(StoreKey key, PoolConfigStore expected_store, uint256 store_index)
        external
        override
    {
        _onlyController();

        PoolConfigStore store = _configStore;
        if (store != expected_store) revert IndexMayHaveChanged();

        ConfigBuffer memory buffer = _configStore.read_to_buffer();
        buffer.remove_entry(key, store_index);
        _configStore = PoolConfigStoreLib.store_from_buffer(buffer);

        delete _unlockedFees[key];
    }

    function batchUpdatePools(PoolConfigStore expected_store, ConfigEntryUpdate[] calldata updates)
        external
        override
    {
        _onlyController();

        PoolConfigStore store = _configStore;
        if (store != expected_store) revert IndexMayHaveChanged();

        ConfigBuffer memory buffer = _configStore.read_to_buffer(0);

        for (uint256 i = 0; i < updates.length; i++) {
            ConfigEntryUpdate calldata update = updates[i];
            buffer.entries[update.index] =
                buffer.get(update.key, update.index).setBundleFee(update.bundleFee);

            update.unlockedFee.validate();
            update.protocolUnlockedFee.validate();

            _unlockedFees[update.key] = UnlockedFees({
                unlockedFee: update.unlockedFee,
                protocolUnlockedFee: update.protocolUnlockedFee
            });
        }

        _configStore = PoolConfigStoreLib.store_from_buffer(buffer);
    }

    /// @dev Function to allow controller to pull an arbitrary amount of tokens from the contract.
    /// Assumed to be accrued validator fees.
    function pullFee(address asset, uint256 amount) external override {
        _onlyController();
        asset.safeTransfer(msg.sender, amount);
    }

    function toggleNodes(address[] calldata nodes) external override {
        _onlyController();
        for (uint256 i = 0; i < nodes.length; i++) {
            address node = nodes[i];
            _isNode[node] = !_isNode[node];
        }
    }

    function unlockWithEmptyAttestation(address node, bytes calldata signature) public {
        if (_isUnlocked()) revert OnlyOncePerBlock();
        if (!_isNode[node]) revert NotNode();

        bytes32 attestationStructHash;
        assembly ("memory-safe") {
            mstore(0x00, ATTEST_EMPTY_BLOCK_TYPE_HASH)
            mstore(0x20, number())
            attestationStructHash := keccak256(0x00, 0x40)
        }

        bytes32 digest = _hashTypedData(attestationStructHash);
        if (!SignatureCheckerLib.isValidSignatureNowCalldata(node, digest, signature)) {
            revert InvalidSignature();
        }

        _lastBlockUpdated = SafeCastLib.toUint64(block.number);
    }

    function _isUnlocked() internal view returns (bool) {
        return _lastBlockUpdated == block.number;
    }

    function _onlyController() internal view {
        if (msg.sender != _controller) revert NotController();
    }

    /// @dev Validates that the caller is a node and that the last call is at least 1 block old.
    /// Blocks reentrant calls as well as separate calls in the same block.
    function _nodeBundleLock() internal {
        if (_lastBlockUpdated == block.number) revert OnlyOncePerBlock();
        if (!_isNode[msg.sender]) revert NotNode();
        _lastBlockUpdated = SafeCastLib.toUint64(block.number);
    }
}
