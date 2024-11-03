// SPDX-License-Identifier: BUSL-1.1
pragma solidity ^0.8.0;

import {GrowthOutsideUpdater} from "./GrowthOutsideUpdater.sol";
import {UniConsumer} from "./UniConsumer.sol";
import {Settlement} from "./Settlement.sol";
import {TopLevelAuth} from "./TopLevelAuth.sol";
import {IBeforeAddLiquidityHook, IBeforeRemoveLiquidityHook} from "../interfaces/IHooks.sol";

import {PairArray} from "../types/Pair.sol";
import {CalldataReader} from "../types/CalldataReader.sol";
import {PoolRewards} from "../types/PoolRewards.sol";
import {Positions, Position} from "../types/Positions.sol";
import {SwapCall, SwapCallLib} from "../types/SwapCall.sol";
import {PoolUpdateVariantMap} from "../types/PoolUpdateVariantMap.sol";

import {SignedUnsignedLib} from "super-sol/libraries/SignedUnsignedLib.sol";
import {SafeTransferLib} from "solady/src/utils/SafeTransferLib.sol";
import {IUniV4} from "../interfaces/IUniV4.sol";
import {IPoolManager} from "../interfaces/IUniV4.sol";
import {PoolId} from "v4-core/src/types/PoolId.sol";
import {PoolKey} from "v4-core/src/types/PoolKey.sol";
import {Currency} from "v4-core/src/types/Currency.sol";
import {FixedPointMathLib} from "solady/src/utils/FixedPointMathLib.sol";
import {X128MathLib} from "../libraries/X128MathLib.sol";
import {SafeCastLib} from "solady/src/utils/SafeCastLib.sol";

/// @author philogy <https://github.com/philogy>
/// @dev Top-level entry point for updating any state related to the underlying hooked Uniswap V4
/// pools. Updates individual positions rewards, initiates swaps and reward distribution.
abstract contract PoolUpdates is
    UniConsumer,
    GrowthOutsideUpdater,
    TopLevelAuth,
    Settlement,
    IBeforeAddLiquidityHook,
    IBeforeRemoveLiquidityHook
{
    using SafeTransferLib for address;
    using X128MathLib for uint256;
    using SafeCastLib for uint256;

    using IUniV4 for IPoolManager;
    using SignedUnsignedLib for *;

    Positions internal positions;
    mapping(PoolId id => PoolRewards) internal poolRewards;

    /// @dev Maintain reward growth & `poolRewards` values such that no one's owed rewards change.
    function beforeAddLiquidity(
        address sender,
        PoolKey calldata key,
        IPoolManager.ModifyLiquidityParams calldata params,
        bytes calldata
    ) external override returns (bytes4) {
        _onlyUniV4();

        PoolId id = _toId(key);
        PoolRewards storage rewards = poolRewards[id];

        uint256 growthInside;
        {
            int24 currentTick = UNI_V4.getSlot0(id).tick();
            uint256 lowerGrowth = rewards.rewardGrowthOutside[uint24(params.tickLower)];
            uint256 upperGrowth = rewards.rewardGrowthOutside[uint24(params.tickUpper)];

            if (currentTick < params.tickLower) {
                unchecked {
                    growthInside = lowerGrowth - upperGrowth;
                }
            } else if (params.tickUpper <= currentTick) {
                // Following Uniswap's convention, if tick is below and uninitialized initialize growth
                // outside to global accumulator.
                if (!UNI_V4.isInitialized(id, params.tickLower, key.tickSpacing)) {
                    rewards.rewardGrowthOutside[uint24(params.tickLower)] =
                        lowerGrowth = rewards.globalGrowth;
                }
                if (!UNI_V4.isInitialized(id, params.tickUpper, key.tickSpacing)) {
                    rewards.rewardGrowthOutside[uint24(params.tickUpper)] =
                        upperGrowth = rewards.globalGrowth;
                }
                unchecked {
                    growthInside = upperGrowth - lowerGrowth;
                }
            } else {
                if (!UNI_V4.isInitialized(id, params.tickLower, key.tickSpacing)) {
                    rewards.rewardGrowthOutside[uint24(params.tickLower)] =
                        lowerGrowth = rewards.globalGrowth;
                }
                unchecked {
                    growthInside = rewards.globalGrowth - lowerGrowth - upperGrowth;
                }
            }
        }

        (Position storage position, bytes32 positionKey) =
            positions.get(id, sender, params.tickLower, params.tickUpper, params.salt);

        uint128 lastLiquidity = UNI_V4.getPositionLiquidity(id, positionKey);
        uint128 liquidityDelta = uint128(uint256(params.liquidityDelta));
        uint128 newLiquidity = lastLiquidity + liquidityDelta;

        if (lastLiquidity == 0) {
            position.lastGrowthInside = growthInside;
        } else {
            // We want to update `lastGrowthInside` such that any previously accrued rewards are
            // preserved:
            // rewards' == rewards
            // (growth_inside - last') * L' = (growth_inside - last) * L
            //  growth_inside - last' = (growth_inside - last) * L / L'
            // last' = growth_inside - (growth_inside - last) * L / L'
            unchecked {
                uint256 lastGrowthAdjustment = FixedPointMathLib.fullMulDiv(
                    growthInside - position.lastGrowthInside, lastLiquidity, newLiquidity
                );
                position.lastGrowthInside = growthInside - lastGrowthAdjustment;
            }
        }

        return this.beforeAddLiquidity.selector;
    }

    function beforeRemoveLiquidity(
        address sender,
        PoolKey calldata key,
        IPoolManager.ModifyLiquidityParams calldata params,
        bytes calldata
    ) external override returns (bytes4) {
        _onlyUniV4();

        unchecked {
            PoolId id = _toId(key);
            (Position storage position, bytes32 positionKey) =
                positions.get(id, sender, params.tickLower, params.tickUpper, params.salt);
            int24 currentTick = UNI_V4.getSlot0(id).tick();
            uint256 growthInside =
                poolRewards[id].getGrowthInside(currentTick, params.tickLower, params.tickUpper);

            uint128 positionTotalLiquidity = UNI_V4.getPositionLiquidity(id, positionKey);
            uint256 rewards = X128MathLib.fullMulX128(
                growthInside - position.lastGrowthInside, positionTotalLiquidity
            );

            if (rewards > 0) {
                // Pay rewards to owner via uniswap delta => assumes that router is not malicious.
                UNI_V4.sync(key.currency0);
                Currency.unwrap(key.currency0).safeTransfer(address(UNI_V4), rewards);
                UNI_V4.settleFor(sender);

                position.lastGrowthInside = growthInside;
            }
        }

        return this.beforeRemoveLiquidity.selector;
    }

    function _updatePools(CalldataReader reader, PairArray pairs)
        internal
        returns (CalldataReader)
    {
        CalldataReader end;
        (reader, end) = reader.readU24End();
        SwapCall memory swapCall = SwapCallLib.newSwapCall(address(this));
        while (reader != end) {
            reader = _updatePool(reader, swapCall, pairs);
        }

        return reader;
    }

    function _updatePool(CalldataReader reader, SwapCall memory swapCall, PairArray pairs)
        internal
        returns (CalldataReader)
    {
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
        bundleDeltas.sub(swapCall.asset0, rewardTotal);

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
