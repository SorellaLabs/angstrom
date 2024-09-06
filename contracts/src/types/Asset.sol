// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import {CalldataReader} from "./CalldataReader.sol";
import {StructArrayLib} from "../libraries/StructArrayLib.sol";

import {console} from "forge-std/console.sol";
import {DEBUG_LOGS} from "../modules/DevFlags.sol";

type Asset is uint256;

type AssetArray is uint256;

using AssetLib for Asset global;
using AssetLib for AssetArray global;

/// @author philogy <https://github.com/philogy>
library AssetLib {
    using StructArrayLib for uint256;

    error AssetsOutOfOrderOrNotUnique();

    /// @dev Size of a single encoded asset (b20:addr ++ b16:borrow ++ b16:save ++ b16:settle)
    uint256 internal constant ASSET_BYTES = 68;

    uint256 internal constant ADDR_OFFSET = 0;
    uint256 internal constant BORROW_OFFSET = 20;
    uint256 internal constant SAVE_OFFSET = 36;
    uint256 internal constant SETTLE_OFFSET = 52;

    function readFromAndValidate(CalldataReader reader) internal pure returns (CalldataReader, AssetArray) {
        uint256 packed;
        (reader, packed) = StructArrayLib.readPackedFrom(reader, ASSET_BYTES);
        return (reader, AssetArray.wrap(packed)._validated());
    }

    function len(AssetArray assets) internal pure returns (uint256) {
        return assets.into().len();
    }

    function _validated(AssetArray self) internal pure returns (AssetArray) {
        uint256 length = self.len();
        address lastAddr = address(0);
        for (uint256 i = 0; i < length; i++) {
            address newAddr = self.get(i).addr();
            if (newAddr <= lastAddr) revert AssetsOutOfOrderOrNotUnique();
            lastAddr = newAddr;
        }
        return self;
    }

    function readAssetAddrFrom(AssetArray self, CalldataReader reader)
        internal
        pure
        returns (CalldataReader, address asset)
    {
        uint256 assetIndex;
        (reader, assetIndex) = reader.readU8();
        asset = self.get(assetIndex).addr();
        return (reader, asset);
    }

    function into(AssetArray self) internal pure returns (uint256) {
        return AssetArray.unwrap(self);
    }

    function into(Asset self) internal pure returns (uint256) {
        return Asset.unwrap(self);
    }

    function get(AssetArray self, uint256 index) internal pure returns (Asset asset) {
        if (DEBUG_LOGS) console.log("[Asset] Attempting to retrieve asset[%s] from array", index);
        self.into()._checkBounds(index);
        return Asset.wrap(self.into().ptr() + index * ASSET_BYTES);
    }

    function addr(Asset self) internal pure returns (address) {
        return self.into().readAddressMemberFromPtr(ADDR_OFFSET);
    }

    function borrow(Asset self) internal pure returns (uint256) {
        return self.into().readU128MemberFromPtr(BORROW_OFFSET);
    }

    function save(Asset self) internal pure returns (uint256) {
        return self.into().readU128MemberFromPtr(SAVE_OFFSET);
    }

    function settle(Asset self) internal pure returns (uint256) {
        return self.into().readU128MemberFromPtr(SETTLE_OFFSET);
    }
}
