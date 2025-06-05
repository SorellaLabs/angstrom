// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {IBeforeSwapHook, IAfterSwapHook, SimplePoolKey} from "../interfaces/IHooks.sol";
import {UniConsumer} from "./UniConsumer.sol";
import {TopLevelAuth} from "./TopLevelAuth.sol";
import {PoolConfigStoreLib} from "../libraries/PoolConfigStore.sol";
import {StoreKey, StoreKeyLib} from "../types/StoreKey.sol";

import {IPoolManager} from "v4-core/src/interfaces/IPoolManager.sol";
import {BeforeSwapDelta} from "v4-core/src/types/BeforeSwapDelta.sol";
import {BalanceDelta} from "v4-core/src/types/BalanceDelta.sol";
import {LPFeeLibrary} from "v4-core/src/libraries/LPFeeLibrary.sol";

/// @author philogy <https://github.com/philogy>
/// @author Will Smith <https://github.com/Will-Smith11>
abstract contract UnlockHook is UniConsumer, TopLevelAuth, IBeforeSwapHook, IAfterSwapHook {
    error UnlockDataTooShort();
    error CannotSwapWhileLocked();

    function beforeSwap(
        address,
        SimplePoolKey calldata key,
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

        StoreKey storeKey = StoreKeyLib.keyFromAssetsUnchecked(key.asset0, key.asset1);

        swapFee = _unlockedFees[storeKey].unlockedFee | LPFeeLibrary.OVERRIDE_FEE_FLAG;

        return (IBeforeSwapHook.beforeSwap.selector, BeforeSwapDelta.wrap(0), swapFee);
    }

    function afterSwap(
        address,
        SimplePoolKey calldata key,
        IPoolManager.SwapParams calldata params,
        BalanceDelta swap_delta,
        bytes calldata
    ) external returns (bytes4, int128) {
        _onlyUniV4();

        StoreKey storeKey = StoreKeyLib.keyFromAssetsUnchecked(key.asset0, key.asset1);
        int24 fee_rate_e6 = int24(_unlockedFees[storeKey].protocolUnlockedFee);
        bool exactIn = params.amountSpecified < 0;

        int128 target_amount =
            exactIn != params.zeroForOne ? swap_delta.amount0() : swap_delta.amount1();
        int128 fee = ((target_amount < 0 ? -target_amount : target_amount) * fee_rate_e6) / 1e6;

        UNI_V4.mint(
            address(FEE_COLLECTOR),
            uint160(exactIn != params.zeroForOne ? key.asset0 : key.asset1),
            uint128(fee)
        );

        return (IAfterSwapHook.afterSwap.selector, fee);
    }
}
