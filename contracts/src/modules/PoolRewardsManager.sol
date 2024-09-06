// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {RewardsUpdater} from "./RewardsUpdater.sol";
import {UniConsumer} from "./UniConsumer.sol";
import {ILiqChangeHooks} from "../interfaces/ILiqChangeHooks.sol";

import {tuint256} from "transient-goodies/TransientPrimitives.sol";
import {AssetArray} from "../types/Asset.sol";
import {CalldataReader} from "../types/CalldataReader.sol";
import {PoolRewards} from "../types/PoolRewards.sol";

import {IUniV4} from "../interfaces/IUniV4.sol";
import {Hooks} from "v4-core/src/libraries/Hooks.sol";
import {ConversionLib} from "../libraries/ConversionLib.sol";
import {IPoolManager} from "../interfaces/IUniV4.sol";
import {PoolId, PoolIdLibrary} from "v4-core/src/types/PoolId.sol";
import {PoolKey} from "v4-core/src/types/PoolKey.sol";
import {BalanceDelta} from "v4-core/src/types/BalanceDelta.sol";
import {MixedSignLib} from "../libraries/MixedSignLib.sol";

import {console} from "forge-std/console.sol";
import {DEBUG_LOGS} from "./DevFlags.sol";

/// @author philogy <https://github.com/philogy>
abstract contract PoolRewardsManager is RewardsUpdater, ILiqChangeHooks, UniConsumer {
    using PoolIdLibrary for PoolKey;
    using IUniV4 for IPoolManager;
    using MixedSignLib for uint128;

    struct Position {
        uint256 rewardDebt;
    }

    mapping(PoolId id => mapping(uint208 positionKey => Position)) positions;
    mapping(PoolId id => PoolRewards) internal poolsRewards;

    constructor() {
        _checkHookPermissions(Hooks.BEFORE_ADD_LIQUIDITY_FLAG | Hooks.AFTER_REMOVE_LIQUIDITY_FLAG);
    }

    // function beforeAddLiquidity(
    //     address sender,
    //     PoolKey calldata key,
    //     IPoolManager.ModifyLiquidityParams calldata params,
    //     bytes calldata hookData
    // ) external override onlyUniV4 returns (bytes4) {
    //     assert(false); // TODO
    //     PoolId id = key.toId();
    //     Position storage position = pools[id].positions[sender][params.tickLower][params.tickUpper];
    //     return this.beforeAddLiquidity.selector;
    // }
    function beforeAddLiquidity(address, PoolKey calldata, IPoolManager.ModifyLiquidityParams calldata, bytes calldata)
        external
        view
        override
        onlyUniV4
        returns (bytes4)
    {
        return this.beforeAddLiquidity.selector;
    }

    // function afterRemoveLiquidity(
    //     address sender,
    //     PoolKey calldata key,
    //     IPoolManager.ModifyLiquidityParams calldata params,
    //     BalanceDelta delta,
    //     bytes calldata hookData
    // ) external returns (bytes4, BalanceDelta) {
    //     assert(false); // TODO
    // }
    function afterRemoveLiquidity(
        address,
        PoolKey calldata,
        IPoolManager.ModifyLiquidityParams calldata,
        BalanceDelta,
        bytes calldata
    ) external pure returns (bytes4, BalanceDelta) {
        return (bytes4(0), BalanceDelta.wrap(0));
    }

    function _rewardPools(CalldataReader reader, AssetArray assets, mapping(address => tuint256) storage freeBalance)
        internal
        returns (CalldataReader)
    {
        CalldataReader end;
        (reader, end) = reader.readU24End();
        while (reader != end) {
            address asset;
            uint256 total;
            (reader, asset, total) = _rewardPool(reader, assets);
            freeBalance[asset].dec(total);
        }

        return reader;
    }

    function _rewardPool(CalldataReader reader, AssetArray assets)
        internal
        returns (CalldataReader, address, uint256 total)
    {
        if (DEBUG_LOGS) console.log("[PoolRewardsManager] entering _rewardPool");
        address asset0;
        PoolId id;
        if (DEBUG_LOGS) console.log("[PoolRewardsManager] decoding asset indices");
        uint16 indexA;
        (reader, indexA) = reader.readU16();
        uint16 indexB;
        (reader, indexB) = reader.readU16();
        if (DEBUG_LOGS) console.log("[PoolRewardsManager] retrieving assets, building pool id");
        asset0 = assets.get(indexA).addr();
        id = ConversionLib.toPoolKey(address(this), asset0, assets.get(indexB).addr()).toId();

        (reader, total) = _decodeAndReward(poolsRewards[id], id, reader);

        return (reader, asset0, total);
    }

    function _toPosKey(address owner, int24 tickLower, int24 tickUpper) internal pure returns (uint208 key) {
        key = uint208(
            uint256(uint160(owner)) | (uint256(uint24(tickLower)) << 160) | (uint256(uint24(tickUpper)) << (160 + 24))
        );
    }

    function _getPoolBitmapInfo(PoolId id, int16 wordPos) internal view override returns (uint256) {
        return UNI_V4.getPoolBitmapInfo(id, wordPos);
    }

    function _getNetTickLiquidity(PoolId id, int24 tick) internal view override returns (int128 liquidityNet) {
        (, liquidityNet) = UNI_V4.getTickLiquidity(id, tick);
    }

    function _getCurrentLiquidity(PoolId id) internal view override returns (uint128 liquidity) {
        liquidity = UNI_V4.getPoolLiquidity(id);
    }

    function _getCurrentTick(PoolId id) internal view override returns (int24 tick) {
        tick = UNI_V4.getSlot0(id).tick();
    }
}
