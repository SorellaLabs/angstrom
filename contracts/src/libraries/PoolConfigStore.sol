// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import {ConfigEntry, ENTRY_SIZE, KEY_MASK} from "../types/ConfigEntry.sol";
import {StoreKey} from "../types/StoreKey.sol";
import {ConfigBuffer} from "../types/ConfigBuffer.sol";

type PoolConfigStore is address;

uint256 constant MEMORY_OFFSET_OFFSET = 192;
uint256 constant STORE_ADDR_OFFSET = 32;
uint256 constant SIZE_OFFSET = 0;
uint256 constant SIZE_MASK = 0xffffffff;
uint256 constant STORE_HEADER_SIZE = 1;

using PoolConfigStoreLib for PoolConfigStore global;

using {PoolConfigStore_neq as !=} for PoolConfigStore global;

function PoolConfigStore_neq(PoolConfigStore a, PoolConfigStore b) pure returns (bool neq) {
    return PoolConfigStore.unwrap(a) != PoolConfigStore.unwrap(b);
}

/// @author philogy <https://github.com/philogy>
/// @dev Handles deploying and querying of "pool config store contracts", SSTORE2 contracts that
/// store the list of configured pools in their bytecode for the sake of saving gas (cold contract
/// code read costs `2600 + 3 * n` vs. `2100 * ceil(n / 32)` for storage, where `n` is the number of
/// bytes to be read). Since the pool config is read with every bundle and we expect to very quickly
/// have more than 1 pair leveraging the "more expensive writes for cheaper reads" trade-off is worth it.
library PoolConfigStoreLib {
    PoolConfigStore internal constant NULL_CONFIG_CACHE = PoolConfigStore.wrap(address(0));

    error NoEntry();

    error FailedToDeployNewStore();

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

    /// @dev Copy to the entries from the store to a new buffer. Does
    function read_to_buffer(PoolConfigStore self) internal view returns (ConfigBuffer memory) {
        return self.read_to_buffer(0);
    }

    function read_to_buffer(PoolConfigStore self, uint256 extra_capacity)
        internal
        view
        returns (ConfigBuffer memory buffer)
    {
        uint256 entry_count = self.totalEntries();
        uint256 capacity = entry_count + extra_capacity;

        buffer.reset_and_alloc_capacity(capacity);

        // Copy store contents into buffer.
        ConfigEntry[] memory entries = buffer.entries;
        assembly ("memory-safe") {
            // Copy all the entries from store into the buffer.
            let bytes_to_copy := mul(entry_count, ENTRY_SIZE)
            extcodecopy(self, add(entries, 0x20), STORE_HEADER_SIZE, bytes_to_copy)
        }

        // Safety: We allocated at least `entry_count` capacity and have copied exactly
        // `entry_count` valid entries from the store into the buffer.
        buffer.unsafe_resize(entry_count);
    }

    function store_from_buffer(ConfigBuffer memory buffer)
        internal
        returns (PoolConfigStore new_store)
    {
        ConfigEntry[] memory entries = buffer.entries;
        uint256 entry_count = entries.length;

        assembly ("memory-safe") {
            // Temporarily overwrite `entries.length` with deployer bytecode.
            mstore(entries, STORE_DEPLOYER)
            // Create store contract.
            new_store :=
                create(
                    0,
                    add(entries, sub(0x20, STORE_DEPLOYER_BYTES)),
                    add(STORE_DEPLOYER_BYTES, mul(entry_count, ENTRY_SIZE))
                )
            // Reset length to previous value.
            mstore(entries, entry_count)
        }

        /// Verify that the deployment was successful.
        if (PoolConfigStore.unwrap(new_store) == address(0)) {
            revert FailedToDeployNewStore();
        }
    }

    function totalEntries(PoolConfigStore self) internal view returns (uint256) {
        return PoolConfigStore.unwrap(self).code.length / ENTRY_SIZE;
    }

    function getWithDefaultEmpty(PoolConfigStore self, StoreKey key, uint256 index)
        internal
        view
        returns (ConfigEntry entry)
    {
        assembly ("memory-safe") {
            // Copy from store into scratch space.
            extcodecopy(self, 0x00, add(STORE_HEADER_SIZE, mul(ENTRY_SIZE, index)), ENTRY_SIZE)
            // Zero out entry if keys do not match.
            entry := mload(0x00)
            entry := mul(entry, eq(key, and(entry, KEY_MASK)))
        }
    }

    function get(PoolConfigStore self, StoreKey key, uint256 index)
        internal
        view
        returns (int24 tickSpacing, uint24 bundleFee)
    {
        ConfigEntry entry = self.getWithDefaultEmpty(key, index);
        if (entry.isEmpty()) revert NoEntry();
        tickSpacing = int24(uint24(entry.tickSpacing()));
        bundleFee = entry.bundleFee();
    }

    function into(PoolConfigStore self) internal pure returns (address) {
        return PoolConfigStore.unwrap(self);
    }
}
