// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {MockERC20} from "../mocks/MockERC20.sol";
import {IPoolManager} from "v4-core/src/interfaces/IPoolManager.sol";
import {IUnlockCallback} from "v4-core/src/interfaces/callback/IUnlockCallback.sol";
import {Angstrom} from "../../src/Angstrom.sol";
import {PoolKey} from "v4-core/src/types/PoolKey.sol";
import {IUniV4} from "../../src/interfaces/IUniV4.sol";
import {PoolIdLibrary} from "v4-core/src/types/PoolId.sol";
import {Slot0} from "v4-core/src/types/Slot0.sol";
import {ConversionLib} from "../../src/libraries/ConversionLib.sol";
import {BalanceDelta} from "v4-core/src/types/BalanceDelta.sol";

/// @author philogy <https://github.com/philogy>
contract TestnetHub is IUnlockCallback {
    using PoolIdLibrary for PoolKey;
    using IUniV4 for IPoolManager;
    using ConversionLib for address;

    error CallFailed(bytes);

    IPoolManager internal immutable UNI_V4;
    address internal immutable ANGSTROM;

    int24 internal constant TICK_SPACING = ConversionLib.TICK_SPACING;

    constructor(address angstrom, address uniV4) {
        UNI_V4 = IPoolManager(uniV4);
        ANGSTROM = angstrom;
    }

    function execute(bytes calldata data) external {
        UNI_V4.unlock(data);
    }

    function unlockCallback(bytes calldata data) external returns (bytes memory) {
        (bool success, bytes memory errData) = address(this).call(data);
        if (!success) revert CallFailed(errData);
        return "";
    }

    function initializePool(address asset0, address asset1, uint160 initialSqrtPriceX96) public {
        PoolKey memory poolKey = ANGSTROM.toPoolKey(asset0, asset1);
        UNI_V4.initialize(poolKey, initialSqrtPriceX96, "");
    }

    function isInitialized(address asset0, address asset1) public view returns (bool) {
        PoolKey memory poolKey = ANGSTROM.toPoolKey(asset0, asset1);
        Slot0 slot0 = UNI_V4.getSlot0(poolKey.toId());
        return slot0.sqrtPriceX96() != 0;
    }

    function modifyLiquidity(address asset0, address asset1, int24 tickLower, int24 tickUpper, int256 liquidityDelta)
        public
    {
        PoolKey memory poolKey = ANGSTROM.toPoolKey(asset0, asset1);
        IPoolManager.ModifyLiquidityParams memory params;
        params.tickLower = tickLower;
        params.tickUpper = tickUpper;
        params.liquidityDelta = liquidityDelta;
        (BalanceDelta callerDelta, BalanceDelta feeDelta) = UNI_V4.modifyLiquidity(poolKey, params, "");
        require(feeDelta.amount0() == 0 && feeDelta.amount1() == 0, "Getting fees?");
        require(callerDelta.amount0() <= 0 && callerDelta.amount1() <= 0, "Receiving money for LP'ing?");

        _settleMintable(asset0, uint128(-callerDelta.amount0()), true);
        _settleMintable(asset1, uint128(-callerDelta.amount1()), true);
    }

    function _settleMintable(address asset, uint256 amount, bool needsSync) internal {
        if (amount > 0) {
            if (needsSync) UNI_V4.sync(asset.intoC());
            MockERC20(asset).mint(address(UNI_V4), amount);
            UNI_V4.settle(asset.intoC());
        }
    }
}
