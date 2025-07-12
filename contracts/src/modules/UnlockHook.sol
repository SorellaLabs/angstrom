// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {IBeforeSwapHook, IAfterSwapHook} from "../interfaces/IHooks.sol";
import {UniConsumer} from "./UniConsumer.sol";
import {TopLevelAuth} from "./TopLevelAuth.sol";
import {PoolConfigStoreLib} from "../libraries/PoolConfigStore.sol";
import {StoreKey, StoreKeyLib} from "../types/StoreKey.sol";
import {tint256} from "transient-goodies/TransientPrimitives.sol";

import {IUniV4, IPoolManager} from "../interfaces/IUniV4.sol";
import {BeforeSwapDelta} from "v4-core/src/types/BeforeSwapDelta.sol";
import {BalanceDelta} from "v4-core/src/types/BalanceDelta.sol";
import {PoolKey} from "v4-core/src/types/PoolKey.sol";
import {LPFeeLibrary} from "v4-core/src/libraries/LPFeeLibrary.sol";
import {PoolId} from "v4-core/src/types/PoolId.sol";

import {PoolUpdates} from "./PoolUpdates.sol";

/// @author philogy <https://github.com/philogy>
/// @author Will Smith <https://github.com/Will-Smith11>
abstract contract UnlockHook is
    UniConsumer,
    TopLevelAuth,
    PoolUpdates,
    IBeforeSwapHook,
    IAfterSwapHook
{
    using IUniV4 for IPoolManager;

    error UnlockDataTooShort();
    error CannotSwapWhileLocked();

    tint256 internal currentTickBeforeSwap;

    function beforeSwap(
        address,
        PoolKey calldata key,
        IPoolManager.SwapParams calldata,
        bytes calldata optionalUnlockData
    ) external returns (bytes4 response, BeforeSwapDelta, uint24 swapFee) {
        _onlyUniV4();

        if (!_isUnlocked()) {
            if (optionalUnlockData.length < 20) {
                if (optionalUnlockData.length == 0) {
                    revert CannotSwapWhileLocked();
                }
                revert UnlockDataTooShort();
            } else {
                address node = address(bytes20(optionalUnlockData[:20]));
                bytes calldata signature = optionalUnlockData[20:];
                unlockWithEmptyAttestation(node, signature);
            }
        }

        StoreKey storeKey =
            StoreKeyLib.keyFromAssetsUnchecked(_addr(key.currency0), _addr(key.currency1));

        swapFee = _unlockedFees[storeKey].unlockedFee | LPFeeLibrary.OVERRIDE_FEE_FLAG;

        PoolId id = _toId(key);
        currentTickBeforeSwap.set(UNI_V4.getSlot0(id).tick());

        return (IBeforeSwapHook.beforeSwap.selector, BeforeSwapDelta.wrap(0), swapFee);
    }

    function afterSwap(
        address,
        PoolKey calldata key,
        IPoolManager.SwapParams calldata params,
        BalanceDelta swap_delta,
        bytes calldata
    ) external returns (bytes4, int128) {
        _onlyUniV4();

        int128 fee;
        {
            StoreKey storeKey =
                StoreKeyLib.keyFromAssetsUnchecked(_addr(key.currency0), _addr(key.currency1));
            int24 fee_rate_e6 = int24(_unlockedFees[storeKey].protocolUnlockedFee);
            bool exactIn = params.amountSpecified < 0;

            int128 target_amount =
                exactIn != params.zeroForOne ? swap_delta.amount0() : swap_delta.amount1();
            fee = ((target_amount < 0 ? -target_amount : target_amount) * fee_rate_e6) / 1e6;

            UNI_V4.mint(
                address(FEE_COLLECTOR),
                (exactIn != params.zeroForOne ? key.currency0 : key.currency1).toId(),
                uint128(fee)
            );
        }

        PoolId id = _toId(key);
        int24 currentTick = UNI_V4.getSlot0(id).tick();
        poolRewards[id].updateAfterTickMove(
            id, UNI_V4, int24(currentTickBeforeSwap.get()), currentTick, key.tickSpacing
        );

        return (IAfterSwapHook.afterSwap.selector, fee);
    }
}
