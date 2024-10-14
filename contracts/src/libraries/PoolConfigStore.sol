// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import {
    ConfigEntry,
    ConfigEntryLib,
    ENTRY_SIZE,
    KEY_MASK,
    TICK_SPACING_OFFSET,
    TICK_SPACING_MASK,
    FEE_OFFSET,
    FEE_MASK
} from "../types/ConfigEntry.sol";

type PoolConfigStore is address;

uint256 constant MEMORY_OFFSET_OFFSET = 192;
uint256 constant STORE_ADDR_OFFSET = 32;
uint256 constant SIZE_OFFSET = 0;
uint256 constant SIZE_MASK = 0xffffffff;
uint256 constant STORE_HEADER_SIZE = 1;

/// @dev Left shift in bits to convert the full hash `keccak256(abi.encode(asset0, asset1))` to a store key.
uint256 constant HASH_TO_STORE_KEY_SHIFT = 40;

/// @dev Max fee allowed.
uint24 constant MAX_FEE = 0.2e6;

type StoreKey is bytes27;

using PoolConfigStoreLib for PoolConfigStore global;

/// @author philogy <https://github.com/philogy>
/// @dev Handles deploying and querying of "pool config store contracts", SSTORE2 contracts that
/// store the list of configured pools in their bytecode for the sake of saving gas (cold contract
/// code read costs `2600 + 3 * n` vs. `2100 * ceil(n / 32)` for storage, where `n` is the number of
/// bytes to be read). Since the pool config is read with every bundle and we expect to very quickly
/// have more than 1 pair leveraging the "more expensive writes for cheaper reads" trade-off is worth it.
library PoolConfigStoreLib {
    PoolConfigStore internal constant NULL_CONFIG_CACHE = PoolConfigStore.wrap(address(0));

    error NoEntry();

    error DuplicateAsset();
    error InvalidTickSpacing();
    error FeeAboveMax();
    error FailedToDeployNewStore();
    error InvalidStoreIndex();

    /*
     * @dev Generated from ./StoreDeployer.huff using https://github.com/Philogy/py-huff (commit: 44bbb4b)
     *  PC   OP BYTES   INSTRUCTION   STACK (Top -> Down)          COMMENT
     * ----------------------------------------------------------------------------------------------
     * Constructor Code
     *   0   600b       PUSH1 11      [11]                         Push constructor size
     *   2   38         CODESIZE      [codesize, 11]               Sum of all bytes including runtime
     *   3   03         SUB           [run_size]                   Subtracting to compute the runtime size
     *   4   80         DUP1          [run_size, run_size]         Duplicate for later
     *   5   600b       PUSH1 11      [11, run_size, run_size]     Push constructor size again
     *   7   5f         PUSH0         [0, 11, run_size, run_size]
     *   8   39         CODECOPY      [run_size]                   Copy the runtime code into memory
     *                                                             (`memory[0:0+run_size] = code[11:11+run_size]`)
     *   9   5f         PUSH0         [0, run_size]
     *  10   f3         RETURN        []                           Return runtime from memory as final code
     *                                                             (`runtime = memory[0:runsize]`)
     * Runtime Code
     *   0   00         STOP          []                           Stop execution. Ensure that even if
     *                                                             called the store contract cannot do
     *                                                             anything like SELFDESTRUCTing itself.
     **/
    uint256 internal constant STORE_DEPLOYER = 0x600b380380600b5f395ff300;
    uint256 internal constant STORE_DEPLOYER_BYTES = 12;

    function removeIntoNew(PoolConfigStore previousStore, uint256 storeIndex)
        internal
        returns (PoolConfigStore newStore)
    {
        uint256 total = previousStore.totalEntries();
        if (storeIndex >= total) revert InvalidStoreIndex();
        // If removing last entry, we can return an empty store.
        if (total == 1) return newStore;
        uint256 entriesAfterToBeRemoved = total - storeIndex - 1;
        assembly ("memory-safe") {
            // Get free memory & copy in the entire store's contents.
            let free := mload(0x40)
            // Add deployment code to the front.
            mstore(free, STORE_DEPLOYER)
            // Get offset after store deployer code for entries.
            let entryOffset := add(free, 0x20)
            // Copy the old store into memory.
            let totalEntryBytes := mul(ENTRY_SIZE, total)
            extcodecopy(previousStore, entryOffset, STORE_HEADER_SIZE, totalEntryBytes)
            // Shift all entries in memory after the one to be removed back by one entry.
            let toBeRemovedMemOffset := add(entryOffset, mul(storeIndex, ENTRY_SIZE))
            mcopy(
                toBeRemovedMemOffset,
                add(toBeRemovedMemOffset, 0x20),
                mul(entriesAfterToBeRemoved, ENTRY_SIZE)
            )
            // Deploy store.
            newStore :=
                create(
                    0,
                    add(free, sub(32, STORE_DEPLOYER_BYTES)),
                    add(sub(totalEntryBytes, ENTRY_SIZE), STORE_DEPLOYER_BYTES)
                )
        }
        if (PoolConfigStore.unwrap(newStore) == address(0)) revert FailedToDeployNewStore();
    }

    /// @dev Create a new store from an old when appending or overriding the entry for the given
    /// asset pair.
    function setIntoNew(
        PoolConfigStore previousStore,
        address asset0,
        address asset1,
        uint16 tickSpacing,
        uint24 feeInE6
    ) internal returns (PoolConfigStore newStore) {
        if (asset0 > asset1) (asset0, asset1) = (asset1, asset0);
        if (asset1 == asset0) revert DuplicateAsset();
        if (tickSpacing == 0) revert InvalidTickSpacing();
        if (feeInE6 > MAX_FEE) revert FeeAboveMax();

        // Update store.
        uint256 total = previousStore.totalEntries();
        StoreKey key = PoolConfigStoreLib.keyFromAssetsUnchecked(asset0, asset1);
        assembly ("memory-safe") {
            // Get free memory & copy in the entire store's contents.
            let free := mload(0x40)
            // Add deployment code to the front.
            mstore(free, STORE_DEPLOYER)
            // Get offset after store deployer code for entries.
            let entryOffset := add(free, 0x20)
            // Copy the old store into memory.
            let totalEntryBytes := mul(ENTRY_SIZE, total)
            extcodecopy(previousStore, entryOffset, STORE_HEADER_SIZE, totalEntryBytes)
            // Construct new entry by splicing in the values.
            let newEntry :=
                or(
                    key,
                    or(
                        shl(TICK_SPACING_OFFSET, and(tickSpacing, TICK_SPACING_MASK)),
                        shl(FEE_OFFSET, and(feeInE6, FEE_MASK))
                    )
                )
            // Search pool to see if it was already configured, if so replace the entry.
            let entriesEnd := add(entryOffset, totalEntryBytes)
            for {} lt(entryOffset, entriesEnd) { entryOffset := add(entryOffset, 0x20) } {
                let entry := mload(entryOffset)
                if eq(key, and(entry, KEY_MASK)) {
                    mstore(entryOffset, newEntry)
                    break
                }
            }
            // Increase `totalEntryBytes` by 0x20 if we broke in the loop.
            totalEntryBytes := add(totalEntryBytes, shl(5, eq(entryOffset, entriesEnd)))
            // Append the entry to the end incase we include it (`totalEntryBytes` will ensure we don't
            // if the entry was found & replaced).
            mstore(entriesEnd, newEntry)
            // Deploy store.
            newStore :=
                create(
                    0,
                    add(free, sub(32, STORE_DEPLOYER_BYTES)),
                    add(totalEntryBytes, STORE_DEPLOYER_BYTES)
                )
        }

        if (PoolConfigStore.unwrap(newStore) == address(0)) revert FailedToDeployNewStore();
    }

    function totalEntries(PoolConfigStore self) internal view returns (uint256) {
        return PoolConfigStore.unwrap(self).code.length / ENTRY_SIZE;
    }

    function get(PoolConfigStore self, StoreKey key, uint256 index)
        internal
        view
        returns (int24 tickSpacing, uint24 feeInE6)
    {
        ConfigEntry entry;
        assembly {
            // Copy from store into scratch space.
            extcodecopy(self, 0x00, add(STORE_HEADER_SIZE, mul(ENTRY_SIZE, index)), ENTRY_SIZE)
            // Zero out entry if keys do not match.
            entry := mload(0x00)
            entry := mul(entry, eq(key, and(entry, KEY_MASK)))
        }
        if (entry.isEmpty()) revert NoEntry();
        tickSpacing = entry.tickSpacing();
        feeInE6 = entry.feeInE6();
    }

    /// @dev Computes the `StoreKey` from the inputs. WARN: Does not check that the assets are
    /// sorted and in unique order.
    function keyFromAssetsUnchecked(address asset0, address asset1)
        internal
        pure
        returns (StoreKey key)
    {
        assembly ("memory-safe") {
            mstore(0x00, asset0)
            mstore(0x20, asset1)
            key := shl(HASH_TO_STORE_KEY_SHIFT, keccak256(0x00, 0x40))
        }
    }
}
