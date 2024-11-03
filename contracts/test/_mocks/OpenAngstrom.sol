// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {Angstrom} from "src/Angstrom.sol";
import {IUniV4, IPoolManager} from "src/interfaces/IUniV4.sol";
import {CalldataReader, CalldataReaderLib} from "src/types/CalldataReader.sol";
import {Asset, AssetArray, AssetLib} from "src/types/Asset.sol";
import {PairArray, PairLib} from "src/types/Pair.sol";
import {SwapCall, SwapCallLib} from "src/types/SwapCall.sol";
import {TypedDataHasher, TypedDataHasherLib} from "src/types/TypedDataHasher.sol";
import {ToBOrderBuffer} from "src/types/ToBOrderBuffer.sol";
import {ToBOrderVariantMap} from "src/types/ToBOrderVariantMap.sol";
import {UserOrderBuffer} from "src/types/UserOrderBuffer.sol";
import {UserOrderVariantMap} from "src/types/UserOrderVariantMap.sol";
import {PoolId} from "v4-core/src/types/PoolId.sol";
import {Position} from "src/types/Positions.sol";
import {PoolConfigStore} from "src/libraries/PoolConfigStore.sol";
import {X128MathLib} from "src/libraries/X128MathLib.sol";

import {console} from "forge-std/console.sol";

/// @author philogy <https://github.com/philogy>
contract OpenAngstrom is Angstrom {
    using IUniV4 for IPoolManager;

    constructor(IPoolManager uniV4, address controller) Angstrom(uniV4, controller) {}

    function nodeBundleLock() public {
        _nodeBundleLock();
    }

    /// @custom:pade Asset
    function takeAsset(bytes calldata encodedAsset) public {
        CalldataReader reader = CalldataReaderLib.from(encodedAsset);

        Asset asset = Asset.wrap(reader.offset());
        _take(asset);
        reader = CalldataReader.wrap(reader.offset() + AssetLib.ASSET_CD_BYTES);

        reader.requireAtEndOf(encodedAsset);
    }

    /// @custom:pade (List<Asset>, List<Pair>, PoolUpdate)
    function updatePool(bytes calldata updatePayload) public {
        CalldataReader reader = CalldataReaderLib.from(updatePayload);

        AssetArray assets;
        (reader, assets) = AssetLib.readFromAndValidate(reader);
        PairArray pairs;
        (reader, pairs) = PairLib.readFromAndValidate(reader, assets, _configStore);
        SwapCall memory swapCall = SwapCallLib.newSwapCall(address(this));
        reader = _updatePool(reader, swapCall, pairs);

        reader.requireAtEndOf(updatePayload);
    }

    /// @custom:pade (List<Asset>, List<Pair>, TopOfBlockOrder)
    function validateAndExecuteToBOrder(bytes calldata tobPayload) public {
        CalldataReader reader = CalldataReaderLib.from(tobPayload);

        AssetArray assets;
        (reader, assets) = AssetLib.readFromAndValidate(reader);
        PairArray pairs;
        (reader, pairs) = PairLib.readFromAndValidate(reader, assets, _configStore);

        ToBOrderBuffer memory buffer;
        buffer.init();
        reader = _validateAndExecuteToBOrder(reader, buffer, _erc712Hasher(), pairs);

        reader.requireAtEndOf(tobPayload);
    }

    /// @custom:pade (List<Asset>, List<Pair>, UserOrder)
    function validateAndExecuteUserOrder(bytes calldata userOrderPayload) public {
        CalldataReader reader = CalldataReaderLib.from(userOrderPayload);

        AssetArray assets;
        (reader, assets) = AssetLib.readFromAndValidate(reader);
        PairArray pairs;
        (reader, pairs) = PairLib.readFromAndValidate(reader, assets, _configStore);

        UserOrderBuffer memory buffer;
        reader = _validateAndExecuteUserOrder(reader, buffer, _erc712Hasher(), pairs);

        reader.requireAtEndOf(userOrderPayload);
    }

    /// @dev custom:pade List<Asset>
    function saveAndSettle(bytes calldata assetsPayload) public {
        CalldataReader reader = CalldataReaderLib.from(assetsPayload);

        AssetArray assets;
        (reader, assets) = AssetLib.readFromAndValidate(reader);
        _saveAndSettle(assets);

        reader.requireAtEndOf(assetsPayload);
    }

    function getDelta(address asset) public view returns (int256) {
        return bundleDeltas.deltas[asset].get();
    }

    function configStore() public view returns (PoolConfigStore) {
        return _configStore;
    }

    function balanceOf(address asset, address owner) public view returns (uint256) {
        return _balances[asset][owner];
    }

    struct GetPositionArgs {
        PoolId id;
        address owner;
        int24 lowerTick;
        int24 upperTick;
        bytes32 salt;
    }

    function getPositionRewards(
        PoolId id,
        address owner,
        int24 lowerTick,
        int24 upperTick,
        bytes32 salt
    ) external view returns (uint256) {
        GetPositionArgs memory args = GetPositionArgs(id, owner, lowerTick, upperTick, salt);

        (Position storage position, bytes32 positionKey) =
            positions.get(args.id, args.owner, args.lowerTick, args.upperTick, args.salt);
        uint128 liquidity = UNI_V4.getPositionLiquidity(args.id, positionKey);
        unchecked {
            uint256 growthInside = poolRewards[args.id].getGrowthInside(
                UNI_V4.getSlot0(args.id).tick(), args.lowerTick, args.upperTick
            );
            uint256 netGrowthInside = growthInside - position.lastGrowthInside;
            return X128MathLib.fullMulX128(netGrowthInside, liquidity);
        }
    }

    function getScaledGrowth(
        PoolId id,
        address owner,
        int24 lowerTick,
        int24 upperTick,
        bytes32 salt,
        uint128 liquidity
    ) external view returns (uint256) {
        GetPositionArgs memory args = GetPositionArgs(id, owner, lowerTick, upperTick, salt);

        (Position storage position,) =
            positions.get(args.id, args.owner, args.lowerTick, args.upperTick, args.salt);
        unchecked {
            uint256 growthInside = poolRewards[args.id].getGrowthInside(
                UNI_V4.getSlot0(args.id).tick(), args.lowerTick, args.upperTick
            );
            uint256 netGrowthInside = growthInside - position.lastGrowthInside;
            return X128MathLib.fullMulX128(netGrowthInside, liquidity);
        }
    }
}
