// SPDX-License-Identifier: BUSL-1.1
pragma solidity ^0.8.0;

import {RewardsUpdater} from "./RewardsUpdater.sol";
import {UniConsumer} from "./UniConsumer.sol";
import {IBeforeAddLiquidityHook, IAfterRemoveLiquidityHook} from "../interfaces/IHooks.sol";

import {DeltaTracker} from "../types/DeltaTracker.sol";
import {AssetArray} from "../types/Asset.sol";
import {CalldataReader} from "../types/CalldataReader.sol";
import {PoolRewards} from "../types/PoolRewards.sol";

import {SignedUnsignedLib} from "super-sol/libraries/SignedUnsignedLib.sol";
import {IUniV4} from "../interfaces/IUniV4.sol";
import {Hooks} from "v4-core/src/libraries/Hooks.sol";
import {ConversionLib} from "../libraries/ConversionLib.sol";
import {IPoolManager} from "../interfaces/IUniV4.sol";
import {PoolId, PoolIdLibrary} from "v4-core/src/types/PoolId.sol";
import {PoolKey} from "v4-core/src/types/PoolKey.sol";
import {BalanceDelta} from "v4-core/src/types/BalanceDelta.sol";
import {MixedSignLib} from "../libraries/MixedSignLib.sol";

import {console} from "forge-std/console.sol";
import {FormatLib} from "super-sol/libraries/FormatLib.sol";

/// @author philogy <https://github.com/philogy>
abstract contract PoolUpdateManager is
    RewardsUpdater,
    UniConsumer,
    IBeforeAddLiquidityHook,
    IAfterRemoveLiquidityHook
{
    using FormatLib for *;

    using PoolIdLibrary for PoolKey;
    using IUniV4 for IPoolManager;
    using MixedSignLib for uint128;
    using SignedUnsignedLib for uint256;

    struct Position {
        uint256 rewardDebt;
    }

    /// @dev Uniswap's `MIN_SQRT_RATIO + 1` to pass the limit check.
    uint160 internal constant MIN_SQRT_RATIO = 4295128740;
    /// @dev Uniswap's `MAX_SQRT_RATIO - 1` to pass the limit check.
    uint160 internal constant MAX_SQRT_RATIO = 1461446703485210103287273052203988822378723970341;

    mapping(PoolId id => mapping(uint208 positionKey => Position)) positions;
    mapping(PoolId id => PoolRewards) internal poolRewards;

    constructor() {
        _checkHookPermissions(Hooks.BEFORE_ADD_LIQUIDITY_FLAG | Hooks.AFTER_REMOVE_LIQUIDITY_FLAG);
    }

    function beforeAddLiquidity(address, PoolKey calldata, IPoolManager.ModifyLiquidityParams calldata, bytes calldata)
        external
        view
        override
        onlyUniV4
        returns (bytes4)
    {
        return this.beforeAddLiquidity.selector;
    }

    function afterRemoveLiquidity(
        address,
        PoolKey calldata,
        IPoolManager.ModifyLiquidityParams calldata,
        BalanceDelta,
        bytes calldata
    ) external pure override returns (bytes4, BalanceDelta) {
        return (bytes4(0), BalanceDelta.wrap(0));
    }

    function _updatePools(CalldataReader reader, DeltaTracker storage deltas, AssetArray assets)
        internal
        returns (CalldataReader)
    {
        CalldataReader end;
        (reader, end) = reader.readU24End();
        while (reader != end) {
            reader = _updatePool(reader, deltas, assets);
        }

        return reader;
    }

    function _updatePool(CalldataReader reader, DeltaTracker storage deltas, AssetArray assets)
        internal
        returns (CalldataReader)
    {
        address asset0;
        address asset1;
        bool zeroForOne;
        {
            uint16 assetIndex;
            (reader, assetIndex) = reader.readU16();
            address assetIn = assets.get(assetIndex).addr();
            (reader, assetIndex) = reader.readU16();
            address assetOut = assets.get(assetIndex).addr();
            zeroForOne = assetIn < assetOut;
            (asset0, asset1) = zeroForOne ? (assetIn, assetOut) : (assetOut, assetIn);
        }
        PoolKey memory poolKey = ConversionLib.toPoolKey(address(this), asset0, asset1);
        PoolId id = PoolIdLibrary.toId(poolKey);

        uint256 amountIn;
        (reader, amountIn) = reader.readU128();

        if (amountIn > 0) {
            int24 tickBefore = UNI_V4.getSlot0(id).tick();
            UNI_V4.swap(
                poolKey,
                IPoolManager.SwapParams({
                    zeroForOne: zeroForOne,
                    amountSpecified: amountIn.neg(),
                    sqrtPriceLimitX96: zeroForOne ? MIN_SQRT_RATIO : MAX_SQRT_RATIO
                }),
                ""
            );
            int24 tickAfter = UNI_V4.getSlot0(id).tick();

            poolRewards[id].updateAfterTickMove(id, UNI_V4, tickBefore, tickAfter);
        }

        uint256 rewardTotal;
        (reader, rewardTotal) = _decodeAndReward(reader, poolRewards[id], id);
        deltas.sub(asset0, rewardTotal);

        return reader;
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
