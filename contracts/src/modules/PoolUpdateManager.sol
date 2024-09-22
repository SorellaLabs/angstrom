// SPDX-License-Identifier: BUSL-1.1
pragma solidity ^0.8.0;

import {RewardsUpdater} from "./RewardsUpdater.sol";
import {UniConsumer} from "./UniConsumer.sol";
import {SettlementManager} from "./SettlementManager.sol";
import {IBeforeAddLiquidityHook, IBeforeRemoveLiquidityHook} from "../interfaces/IHooks.sol";

import {DeltaTracker} from "../types/DeltaTracker.sol";
import {AssetArray} from "../types/Asset.sol";
import {CalldataReader} from "../types/CalldataReader.sol";
import {PoolRewards} from "../types/PoolRewards.sol";
import {Positions, Position} from "src/libraries/Positions.sol";

import {SignedUnsignedLib} from "super-sol/libraries/SignedUnsignedLib.sol";
import {IUniV4} from "../interfaces/IUniV4.sol";
import {Hooks} from "v4-core/src/libraries/Hooks.sol";
import {ConversionLib} from "../libraries/ConversionLib.sol";
import {IPoolManager} from "../interfaces/IUniV4.sol";
import {PoolId, PoolIdLibrary} from "v4-core/src/types/PoolId.sol";
import {PoolKey} from "v4-core/src/types/PoolKey.sol";
import {BalanceDelta} from "v4-core/src/types/BalanceDelta.sol";
import {MixedSignLib} from "../libraries/MixedSignLib.sol";
import {Currency} from "v4-core/src/types/Currency.sol";
/// @custom:mounted uint256 (external)
import {FixedPointMathLib} from "solady/src/utils/FixedPointMathLib.sol";
/// @custom:mounted uint256 (external)
import {SafeCastLib} from "solady/src/utils/SafeCastLib.sol";

import {console} from "forge-std/console.sol";
import {FormatLib} from "super-sol/libraries/FormatLib.sol";

/// @author philogy <https://github.com/philogy>
abstract contract PoolUpdateManager is
    RewardsUpdater,
    UniConsumer,
    SettlementManager,
    IBeforeAddLiquidityHook,
    IBeforeRemoveLiquidityHook
{
    using FormatLib for *;

    using SafeCastLib for uint256;

    using IUniV4 for IPoolManager;
    using FixedPointMathLib for uint256;
    using SignedUnsignedLib for *;

    /// @dev Uniswap's `MIN_SQRT_RATIO + 1` to pass the limit check.
    uint160 internal constant MIN_SQRT_RATIO = 4295128740;
    /// @dev Uniswap's `MAX_SQRT_RATIO - 1` to pass the limit check.
    uint160 internal constant MAX_SQRT_RATIO = 1461446703485210103287273052203988822378723970341;

    Positions internal positions;
    mapping(PoolId id => PoolRewards) internal poolRewards;

    constructor() {
        _checkHookPermissions(Hooks.BEFORE_ADD_LIQUIDITY_FLAG | Hooks.BEFORE_REMOVE_LIQUIDITY_FLAG);
    }

    /// @dev Maintain reward growth & `poolRewards` values such that no one's owed rewards change.
    function beforeAddLiquidity(
        address sender,
        PoolKey calldata key,
        IPoolManager.ModifyLiquidityParams calldata params,
        bytes calldata
    ) external override onlyUniV4 returns (bytes4) {
        uint256 liquidityDelta = uint256(params.liquidityDelta);

        PoolId id = ConversionLib.toId(key);
        int24 lowerTick = params.tickLower;
        int24 upperTick = params.tickUpper;
        PoolRewards storage rewards = poolRewards[id];
        (Position storage position,) = positions.get(id, sender, lowerTick, upperTick, params.salt);

        int24 currentTick = UNI_V4.getSlot0(id).tick();

        uint256 lowerGrowth = rewards.rewardGrowthOutside[uint24(lowerTick)];
        uint256 upperGrowth = rewards.rewardGrowthOutside[uint24(upperTick)];

        uint256 newGrowthInside;
        if (currentTick < lowerTick) {
            // Check to maintain invariant that `currentTick < lowerTick -> upperGrowth <= lowerGrowth`
            if (lowerGrowth < upperGrowth) {
                // Should only be the case for newly initialized ticks.
                rewards.rewardGrowthOutside[uint24(lowerTick)] = lowerGrowth = upperGrowth;
            } else {
                newGrowthInside = lowerGrowth - upperGrowth;
            }
        } else if (upperTick <= currentTick) {
            // Check to maintain invariant that `upperTick <= currentTick -> lowerGrowth <= upperGrowth`
            if (upperGrowth < lowerGrowth) {
                // Should only be the case for newly initialized ticks.
                rewards.rewardGrowthOutside[uint24(upperTick)] = upperGrowth = lowerGrowth;
            } else {
                newGrowthInside = upperGrowth - lowerGrowth;
            }
        } else {
            newGrowthInside = rewards.globalGrowth - lowerGrowth - upperGrowth;
        }

        uint256 newPastRewards = newGrowthInside.mulWad(liquidityDelta);
        position.pastRewards += newPastRewards;

        return this.beforeAddLiquidity.selector;
    }

    function beforeRemoveLiquidity(
        address sender,
        PoolKey calldata key,
        IPoolManager.ModifyLiquidityParams calldata params,
        bytes calldata
    ) external override onlyUniV4 returns (bytes4) {
        uint256 liquidityDelta = params.liquidityDelta.neg();

        PoolId id = ConversionLib.toId(key);
        (Position storage position, bytes32 positionKey) =
            positions.get(id, sender, params.tickLower, params.tickUpper, params.salt);
        int24 currentTick = UNI_V4.getSlot0(id).tick();
        uint256 growthInside = poolRewards[id].getGrowthInside(currentTick, params.tickLower, params.tickUpper);

        uint128 positionTotalLiquidity = UNI_V4.getPositionLiquidity(id, positionKey);
        uint256 rewards = growthInside.mulWad(positionTotalLiquidity) - position.pastRewards;

        _settleRewardViaUniswapTo(sender, key.currency0, rewards);

        position.pastRewards = growthInside.mulWad(positionTotalLiquidity - liquidityDelta.toUint128());

        return this.beforeRemoveLiquidity.selector;
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
                    amountSpecified: SignedUnsignedLib.neg(amountIn),
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
