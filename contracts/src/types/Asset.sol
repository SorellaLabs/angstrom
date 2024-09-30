// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import {CalldataReader} from "./CalldataReader.sol";

import {console} from "forge-std/console.sol";

uint256 constant FEE_SUMMARY_ENTRY_SIZE = 36;

type Asset is uint256;

type AssetArray is uint256;

using AssetLib for Asset global;
using AssetLib for AssetArray global;

/// @author philogy <https://github.com/philogy>
library AssetLib {
    error AssetsOutOfOrderOrNotUnique();
    error AssetAccessOutOfBounds(uint256 index, uint256 length);

    /// @dev Size of a single encoded asset (b20:addr ++ b16:save ++ b16:borrow ++ b16:settle)
    uint256 internal constant ASSET_CD_BYTES = 68;

    uint256 internal constant ADDR_OFFSET = 0;
    uint256 internal constant SAVE_OFFSET = 20;
    uint256 internal constant TAKE_OFFSET = 36;
    uint256 internal constant SETTLE_OFFSET = 52;

    uint256 internal constant LENGTH_MASK = 0xffffffff;
    uint256 internal constant CALLDATA_PTR_OFFSET = 32;

    function readFromAndValidate(CalldataReader reader)
        internal
        pure
        returns (CalldataReader, AssetArray assets)
    {
        CalldataReader end;
        (reader, end) = reader.readU24End();

        uint256 length = (end.offset() - reader.offset()) / ASSET_CD_BYTES;

        assets = AssetArray.wrap((reader.offset() << CALLDATA_PTR_OFFSET) | length);

        address lastAddr = address(0);
        for (uint256 i = 0; i < length; i++) {
            address newAddr = assets.getUnchecked(i).addr();
            if (newAddr <= lastAddr) revert AssetsOutOfOrderOrNotUnique();
            lastAddr = newAddr;
        }

        return (end, assets);
    }

    function len(AssetArray assets) internal pure returns (uint256) {
        return AssetArray.unwrap(assets) & LENGTH_MASK;
    }

    function get(AssetArray self, uint256 index) internal pure returns (Asset asset) {
        if (self.len() <= index) revert AssetAccessOutOfBounds(index, self.len());
        return self.getUnchecked(index);
    }

    function getUnchecked(AssetArray self, uint256 index) internal pure returns (Asset asset) {
        unchecked {
            uint256 raw_calldataOffset = AssetArray.unwrap(self) >> CALLDATA_PTR_OFFSET;
            return Asset.wrap(raw_calldataOffset + index * ASSET_CD_BYTES);
        }
    }

    function raw_copyFeeEntryToMemory(Asset self, uint256 raw_memOffset) internal pure {
        assembly ("memory-safe") {
            calldatacopy(raw_memOffset, add(self, ADDR_OFFSET), FEE_SUMMARY_ENTRY_SIZE)
        }
    }

    function addr(Asset self) internal pure returns (address value) {
        assembly {
            value := shr(96, calldataload(add(self, ADDR_OFFSET)))
        }
    }

    function take(Asset self) internal pure returns (uint128 amount) {
        assembly {
            amount := shr(128, calldataload(add(self, TAKE_OFFSET)))
        }
    }

    function save(Asset self) internal pure returns (uint128 amount) {
        assembly {
            amount := shr(128, calldataload(add(self, SAVE_OFFSET)))
        }
    }

    function settle(Asset self) internal pure returns (uint128 amount) {
        assembly {
            amount := shr(128, calldataload(add(self, SETTLE_OFFSET)))
        }
    }
}
