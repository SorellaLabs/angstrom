// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {IUniV4, IPoolManager} from "../../src/interfaces/IUniV4.sol";
import {BalanceDelta, toBalanceDelta} from "v4-core/src/types/BalanceDelta.sol";
import {Currency} from "v4-core/src/types/Currency.sol";
import {IUnlockCallback} from "v4-core/src/interfaces/callback/IUnlockCallback.sol";
import {SafeTransferLib} from "solady/src/utils/SafeTransferLib.sol";
import {PoolKey} from "v4-core/src/types/PoolKey.sol";
import {FormatLib} from "super-sol/libraries/FormatLib.sol";

/// @author philogy <https://github.com/philogy>
/// @notice Likely vulnerable, NOT FOR PRODUCTION USE.
contract RouterActor is IUnlockCallback {
    using FormatLib for *;
    using IUniV4 for IPoolManager;
    using SafeTransferLib for address;

    enum Action {
        Swap,
        Liquidity
    }

    IPoolManager uniV4;

    constructor(IPoolManager uniV4_) {
        uniV4 = uniV4_;
    }

    function swap(
        PoolKey calldata key,
        bool zeroForOne,
        int256 amountSpecified,
        uint160 sqrtPriceLimitX96
    ) external returns (BalanceDelta) {
        bytes memory ret = uniV4.unlock(
            bytes.concat(
                bytes1(uint8(Action.Swap)),
                abi.encode(
                    key, IPoolManager.SwapParams(zeroForOne, amountSpecified, sqrtPriceLimitX96)
                )
            )
        );
        return abi.decode(ret, (BalanceDelta));
    }

    function modifyLiquidity(
        PoolKey calldata key,
        int24 lowerTick,
        int24 upperTick,
        int256 liquidityDelta,
        bytes32 salt
    ) external returns (BalanceDelta, BalanceDelta) {
        bytes memory ret = uniV4.unlock(
            bytes.concat(
                bytes1(uint8(Action.Liquidity)),
                abi.encode(
                    key,
                    IPoolManager.ModifyLiquidityParams(lowerTick, upperTick, liquidityDelta, salt)
                )
            )
        );
        return abi.decode(ret, (BalanceDelta, BalanceDelta));
    }

    function unlockCallback(bytes calldata payload) external returns (bytes memory) {
        require(address(uniV4) == msg.sender);

        Action action = Action(uint8(bytes1(payload[:1])));

        if (action == Action.Swap) {
            (PoolKey memory key, IPoolManager.SwapParams memory params) =
                abi.decode(payload[1:], (PoolKey, IPoolManager.SwapParams));
            return _swap(key, params);
        } else if (action == Action.Liquidity) {
            (PoolKey memory key, IPoolManager.ModifyLiquidityParams memory params) =
                abi.decode(payload[1:], (PoolKey, IPoolManager.ModifyLiquidityParams));
            return _modifyLiquidity(key, params);
        } else {
            revert("Unrecognized action");
        }
    }

    function recycle(address asset) external {
        asset.safeTransferAll(msg.sender);
    }

    function transfer(address asset, address to, uint256 amount) external {
        asset.safeTransfer(to, amount);
    }

    function _swap(PoolKey memory key, IPoolManager.SwapParams memory params)
        internal
        returns (bytes memory)
    {
        BalanceDelta delta = uniV4.swap(key, params, "");
        _settle(key, delta);
        return abi.encode(delta);
    }

    function _modifyLiquidity(PoolKey memory key, IPoolManager.ModifyLiquidityParams memory params)
        internal
        returns (bytes memory)
    {
        (BalanceDelta callerDelta, BalanceDelta feesAccrued) =
            uniV4.modifyLiquidity(key, params, "");

        _settle(key, callerDelta + feesAccrued);
        if (params.liquidityDelta <= 0) {
            int128 rewardDelta =
                int128(uniV4.getDelta(address(this), Currency.unwrap(key.currency0)));
            _settle(key.currency0, rewardDelta);
        }
        return abi.encode(callerDelta, feesAccrued);
    }

    function _settle(PoolKey memory key, BalanceDelta delta) internal {
        _settle(key.currency0, delta.amount0());
        _settle(key.currency1, delta.amount1());
    }

    function _settle(Currency currency, int128 amount) internal {
        unchecked {
            if (amount < 0) {
                uniV4.sync(currency);
                Currency.unwrap(currency).safeTransfer(address(uniV4), uint128(-amount));
                uniV4.settle();
            } else if (0 < amount) {
                uniV4.take(currency, address(this), uint128(amount));
            }
        }
    }
}
