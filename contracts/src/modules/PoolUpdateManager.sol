// SPDX-License-Identifier: BUSL-1.1
pragma solidity ^0.8.0;

import {RewardsUpdater} from "./RewardsUpdater.sol";
import {UniConsumer} from "./UniConsumer.sol";
import {SettlementManager} from "./SettlementManager.sol";
import {NodeManager} from "./NodeManager.sol";
import {IBeforeAddLiquidityHook, IBeforeRemoveLiquidityHook} from "../interfaces/IHooks.sol";

import {DeltaTracker} from "../types/DeltaTracker.sol";
import {PairArray} from "../types/Pair.sol";
import {CalldataReader} from "../types/CalldataReader.sol";
import {PoolRewards} from "../types/PoolRewards.sol";
import {Positions, Position} from "../types/Positions.sol";
import {UniCallLib, UniSwapCallBuffer} from "../libraries/UniCallLib.sol";
import {PoolUpdateVariantMap} from "../types/PoolUpdateVariantMap.sol";

import {SignedUnsignedLib} from "super-sol/libraries/SignedUnsignedLib.sol";
import {IUniV4} from "../interfaces/IUniV4.sol";
import {Hooks} from "v4-core/src/libraries/Hooks.sol";
import {IPoolManager} from "../interfaces/IUniV4.sol";
import {PoolId} from "v4-core/src/types/PoolId.sol";
import {PoolKey} from "v4-core/src/types/PoolKey.sol";
import {MixedSignLib} from "../libraries/MixedSignLib.sol";
import {Currency} from "v4-core/src/types/Currency.sol";
/// @custom:mounted uint256 (external)
import {FixedPointMathLib} from "solady/src/utils/FixedPointMathLib.sol";
/// @custom:mounted uint256 (external)
import {SafeCastLib} from "solady/src/utils/SafeCastLib.sol";
/// @author philogy <https://github.com/philogy>

abstract contract PoolUpdateManager is
    UniConsumer,
    RewardsUpdater,
    SettlementManager,
    NodeManager,
    IBeforeAddLiquidityHook,
    IBeforeRemoveLiquidityHook
{
    using SafeCastLib for uint256;

    using IUniV4 for IPoolManager;
    using FixedPointMathLib for uint256;
    using SignedUnsignedLib for *;

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

        PoolId id = _toId(key);
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

        PoolId id = _toId(key);
        (Position storage position, bytes32 positionKey) =
            positions.get(id, sender, params.tickLower, params.tickUpper, params.salt);
        int24 currentTick = UNI_V4.getSlot0(id).tick();
        uint256 growthInside =
            poolRewards[id].getGrowthInside(currentTick, params.tickLower, params.tickUpper);

        uint128 positionTotalLiquidity = UNI_V4.getPositionLiquidity(id, positionKey);
        uint256 rewards = growthInside.mulWad(positionTotalLiquidity) - position.pastRewards;

        _settleRewardViaUniswapTo(sender, key.currency0, rewards);

        position.pastRewards =
            growthInside.mulWad(positionTotalLiquidity - liquidityDelta.toUint128());

        return this.beforeRemoveLiquidity.selector;
    }

    function _updatePools(CalldataReader reader, DeltaTracker storage deltas, PairArray pairs)
        internal
        returns (CalldataReader)
    {
        CalldataReader end;
        (reader, end) = reader.readU24End();
        UniSwapCallBuffer memory swapCall = UniCallLib.newSwapCall(address(this));
        while (reader != end) {
            reader = _updatePool(reader, swapCall, deltas, pairs);
        }

        return reader;
    }

    function _updatePool(
        CalldataReader reader,
        UniSwapCallBuffer memory swapCall,
        DeltaTracker storage deltas,
        PairArray pairs
    ) internal returns (CalldataReader) {
        PoolUpdateVariantMap variantMap;
        {
            uint8 variantByte;
            (reader, variantByte) = reader.readU8();
            variantMap = PoolUpdateVariantMap.wrap(variantByte);
        }
        swapCall.setZeroForOne(variantMap.zeroForOne());
        uint16 pairIndex;
        (reader, pairIndex) = reader.readU16();
        (swapCall.asset0, swapCall.asset1, swapCall.tickSpacing) =
            pairs.get(pairIndex).getPoolInfo();

        PoolId id = swapCall.getId();

        uint256 amountIn;
        (reader, amountIn) = reader.readU128();

        int24 currentTick;
        if (amountIn > 0) {
            int24 tickBefore = UNI_V4.getSlot0(id).tick();
            swapCall.amountSpecified = SignedUnsignedLib.neg(amountIn);
            // The swap delta is tracked on Uniswap's side so we don't need to here. It's accounted for in the asset
            // take & settle steps.
            swapCall.call(UNI_V4);
            currentTick = UNI_V4.getSlot0(id).tick();

            poolRewards[id].updateAfterTickMove(
                id, UNI_V4, tickBefore, currentTick, swapCall.tickSpacing
            );
        } else {
            currentTick = UNI_V4.getSlot0(id).tick();
        }

        uint256 rewardTotal;
        (reader, rewardTotal) = _decodeAndReward(
            variantMap.currentOnly(), reader, poolRewards[id], id, swapCall.tickSpacing, currentTick
        );
        deltas.sub(swapCall.asset0, rewardTotal);

        return reader;
    }

    function _toId(PoolKey calldata poolKey) internal pure returns (PoolId id) {
        assembly ("memory-safe") {
            let ptr := mload(0x40)
            calldatacopy(ptr, poolKey, mul(32, 5))
            id := keccak256(ptr, mul(32, 5))
        }
    }
}
