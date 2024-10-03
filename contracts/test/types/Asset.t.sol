// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {Test} from "forge-std/Test.sol";
import {Asset as RefAsset, AssetLib as RefAssetLib} from "test/_reference/Asset.sol";
import {CalldataReader, CalldataReaderLib} from "src/types/CalldataReader.sol";
import {Asset, AssetArray, AssetLib} from "src/types/Asset.sol";

import {console} from "forge-std/console.sol";

/// @author philogy <https://github.com/philogy>
contract AssetTest is Test {
    using RefAssetLib for RefAsset[];

    function setUp() public {}

    function test_fuzzing_referenceAssetEncoding(
        address addr,
        uint128 take,
        uint128 save,
        uint128 settle
    ) public pure {
        assertEq(
            RefAsset({addr: addr, take: take, save: save, settle: settle}).encode(),
            abi.encodePacked(addr, save, take, settle)
        );
    }

    function test_fuzzing_sortedArrayIsAccepted(RefAsset[] memory assets) public view {
        vm.assume(assets.length <= type(uint24).max / AssetLib.ASSET_CD_BYTES);
        assets.sort();
        vm.assume(_uniqueAndOrdered(assets));
        bytes memory data = assets.encode();
        this._fuzzing_sortedArrayIsAccepted(data, assets);
    }

    function _fuzzing_sortedArrayIsAccepted(bytes calldata data, RefAsset[] calldata inputAssets)
        external
        pure
    {
        CalldataReader reader = CalldataReaderLib.from(data);
        (, AssetArray encodedAssets) = AssetLib.readFromAndValidate(reader);
        assertEq(encodedAssets.len(), inputAssets.length);
        for (uint256 i = 0; i < inputAssets.length; i++) {
            RefAsset calldata inpAsset = inputAssets[i];
            Asset encAsset = encodedAssets.get(i);
            assertEq(inpAsset.addr, encAsset.addr());
            assertEq(inpAsset.take, encAsset.take());
            assertEq(inpAsset.save, encAsset.save());
            assertEq(inpAsset.settle, encAsset.settle());
        }
    }

    function test_fuzzing_revertsOutOfBoundAccess(RefAsset[] memory assets, uint256 index) public {
        vm.assume(assets.length <= type(uint24).max / AssetLib.ASSET_CD_BYTES);
        assets.sort();
        vm.assume(_uniqueAndOrdered(assets));
        index = bound(index, assets.length, type(uint256).max);
        bytes memory data = assets.encode();
        this._fuzzing_revertsOutOfBoundAccess(data, index);
    }

    function _fuzzing_revertsOutOfBoundAccess(bytes calldata data, uint256 index) external {
        CalldataReader reader = CalldataReaderLib.from(data);
        (, AssetArray encodedAssets) = AssetLib.readFromAndValidate(reader);
        vm.expectRevert(
            abi.encodeWithSelector(
                AssetLib.AssetAccessOutOfBounds.selector, index, encodedAssets.len()
            )
        );
        encodedAssets.get(index);
    }

    function test_revertsDuplicateAsset() public {
        address asset1 = makeAddr("asset_1");
        address asset2 = makeAddr("asset_2");
        (asset1, asset2) = asset1 < asset2 ? (asset1, asset2) : (asset2, asset1);

        RefAsset[] memory assets = new RefAsset[](3);
        assets[0].addr = asset1;
        assets[1].addr = asset1;
        assets[2].addr = asset2;

        assertAssetsNotUniqueOrdered(assets);
    }

    function test_revertsContainsZero() public {
        address asset1 = makeAddr("asset_1");
        address asset2 = makeAddr("asset_2");
        (asset1, asset2) = asset1 < asset2 ? (asset1, asset2) : (asset2, asset1);

        RefAsset[] memory assets = new RefAsset[](3);
        assets[0].addr = address(0);
        assets[1].addr = asset1;
        assets[2].addr = asset2;

        assertAssetsNotUniqueOrdered(assets);
    }

    function test_fuzzing_revertsDuplicateAssets(
        RefAsset[] memory assets,
        uint256 duplicateFromIndex,
        uint256 duplicateToIndex
    ) public {
        vm.assume(assets.length <= type(uint24).max / AssetLib.ASSET_CD_BYTES);
        vm.assume(assets.length >= 2);
        assets.sort();
        vm.assume(_uniqueAndOrdered(assets));
        duplicateFromIndex = bound(duplicateFromIndex, 0, assets.length - 1);
        duplicateToIndex =
            (bound(duplicateToIndex, 1, assets.length - 1) + duplicateFromIndex) % assets.length;
        assertTrue(duplicateFromIndex != duplicateToIndex);

        assets[duplicateToIndex].addr = assets[duplicateFromIndex].addr;
        vm.expectRevert(AssetLib.AssetsOutOfOrderOrNotUnique.selector);
        this._revertsDuplicateAssets(assets.encode());
    }

    function _revertsDuplicateAssets(bytes calldata data) external pure {
        CalldataReader reader = CalldataReaderLib.from(data);
        AssetLib.readFromAndValidate(reader);
    }

    function assertAssetsNotUniqueOrdered(RefAsset[] memory assets) internal {
        bytes memory data = assets.encode();
        vm.expectRevert(AssetLib.AssetsOutOfOrderOrNotUnique.selector);
        this._assertAssetsNotUniqueOrdered(data);
    }

    function _assertAssetsNotUniqueOrdered(bytes calldata data) external pure {
        CalldataReader reader = CalldataReaderLib.from(data);
        AssetLib.readFromAndValidate(reader);
    }

    function _uniqueAndOrdered(RefAsset[] memory assets) internal pure returns (bool) {
        address lastAsset = address(0);
        for (uint256 i = 0; i < assets.length; i++) {
            RefAsset memory asset = assets[i];
            if (asset.addr <= lastAsset) return false;
            lastAsset = asset.addr;
        }
        return true;
    }
}
