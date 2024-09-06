// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {MockERC20} from "super-sol/mocks/MockERC20.sol";
import {IPoolManager} from "v4-core/src/interfaces/IPoolManager.sol";
import {IUnlockCallback} from "v4-core/src/interfaces/callback/IUnlockCallback.sol";
import {Angstrom} from "../../src/Angstrom.sol";
import {PoolKey} from "v4-core/src/types/PoolKey.sol";
import {IUniV4} from "../../src/interfaces/IUniV4.sol";
import {PoolIdLibrary} from "v4-core/src/types/PoolId.sol";
import {Slot0} from "v4-core/src/types/Slot0.sol";
import {ConversionLib} from "../../src/libraries/ConversionLib.sol";
import {BalanceDelta} from "v4-core/src/types/BalanceDelta.sol";

import {SignedUnsignedLib} from "super-sol/libraries/SignedUnsignedLib.sol";
import {console2 as console} from "forge-std/console2.sol";
import {FormatLib} from "super-sol/libraries/FormatLib.sol";

/// @author philogy <https://github.com/philogy>
/// @dev Interacts with pools
contract PoolGate is IUnlockCallback {
    using FormatLib for *;
    using SignedUnsignedLib for *;
    using PoolIdLibrary for PoolKey;
    using IUniV4 for IPoolManager;
    using ConversionLib for address;

    error CallFailed(bytes);

    IPoolManager internal immutable UNI_V4;
    address public hook;

    constructor(address uniV4) {
        UNI_V4 = IPoolManager(uniV4);
    }

    function setHook(address hook_) external {
        hook = hook_;
    }

    function initializePool(address asset0, address asset1, uint160 initialSqrtPriceX96) public returns (int24 tick) {
        bytes memory data = UNI_V4.unlock(abi.encodeCall(this.__initializePool, (asset0, asset1, initialSqrtPriceX96)));
        return abi.decode(data, (int24));
    }

    function addLiquidity(address asset0, address asset1, int24 tickLower, int24 tickUpper, uint256 liquidity)
        public
        returns (uint256 amount0, uint256 amount1)
    {
        bytes memory data =
            UNI_V4.unlock(abi.encodeCall(this.__addLiquidity, (asset0, asset1, tickLower, tickUpper, liquidity)));
        (BalanceDelta callerDelta,) = abi.decode(data, (BalanceDelta, BalanceDelta));
        require(callerDelta.amount0() <= 0 && callerDelta.amount1() <= 0, "Amount positive?");
        amount0 = uint128(-callerDelta.amount0());
        amount1 = uint128(-callerDelta.amount1());
    }

    function mint(address asset, uint256 amount) public {
        mint(msg.sender, asset, amount);
    }

    function mint(address to, address asset, uint256 amount) public {
        UNI_V4.unlock(abi.encodeCall(this.__mint, (to, asset, amount)));
    }

    function isInitialized(address asset0, address asset1) public view returns (bool) {
        PoolKey memory poolKey = hook.toPoolKey(asset0, asset1);
        Slot0 slot0 = UNI_V4.getSlot0(poolKey.toId());
        return slot0.sqrtPriceX96() != 0;
    }

    function unlockCallback(bytes calldata data) external returns (bytes memory) {
        (bool success, bytes memory retData) = address(this).call(data);
        if (!success) revert CallFailed(retData);
        return retData;
    }

    function __initializePool(address asset0, address asset1, uint160 initialSqrtPriceX96)
        public
        returns (int24 tick)
    {
        PoolKey memory poolKey = hook.toPoolKey(asset0, asset1);
        tick = UNI_V4.initialize(poolKey, initialSqrtPriceX96, "");
    }

    function __addLiquidity(address asset0, address asset1, int24 tickLower, int24 tickUpper, uint256 liquidity)
        public
        returns (BalanceDelta callerDelta, BalanceDelta feeDelta)
    {
        PoolKey memory poolKey = hook.toPoolKey(asset0, asset1);
        IPoolManager.ModifyLiquidityParams memory params;
        params.tickLower = tickLower;
        params.tickUpper = tickUpper;
        params.liquidityDelta = liquidity.signed();
        (callerDelta, feeDelta) = UNI_V4.modifyLiquidity(poolKey, params, "");
        require(feeDelta.amount0() == 0 && feeDelta.amount1() == 0, "Getting fees?");
        require(callerDelta.amount0() <= 0 && callerDelta.amount1() <= 0, "Receiving money for LP'ing?");

        _settleMintable(asset0, uint128(-callerDelta.amount0()), true);
        _settleMintable(asset1, uint128(-callerDelta.amount1()), true);
    }

    function __mint(address to, address asset, uint256 amount) public {
        uint256 id;
        // forgefmt: disable-next-item
        assembly { id := asset }
        UNI_V4.mint(to, id, amount);
        _settleMintable(asset, amount, true);
    }

    function _settleMintable(address asset, uint256 amount, bool needsSync) internal {
        if (amount > 0) {
            if (needsSync) UNI_V4.sync(asset.intoC());
            MockERC20(asset).mint(address(UNI_V4), amount);
            UNI_V4.settle();
        }
    }
}
