// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {CommonBase} from "forge-std/Base.sol";

import {MockERC20} from "super-sol/mocks/MockERC20.sol";
import {IPoolManager} from "v4-core/src/interfaces/IPoolManager.sol";
import {IUnlockCallback} from "v4-core/src/interfaces/callback/IUnlockCallback.sol";
import {Angstrom} from "../../src/Angstrom.sol";
import {PoolKey} from "v4-core/src/types/PoolKey.sol";
import {IUniV4} from "../../src/interfaces/IUniV4.sol";
import {PoolId, PoolIdLibrary} from "v4-core/src/types/PoolId.sol";
import {Slot0} from "v4-core/src/types/Slot0.sol";
import {ConversionLib} from "../../src/libraries/ConversionLib.sol";
import {Currency} from "v4-core/src/types/Currency.sol";
import {BalanceDelta, toBalanceDelta} from "v4-core/src/types/BalanceDelta.sol";

import {SignedUnsignedLib} from "super-sol/libraries/SignedUnsignedLib.sol";
import {console2 as console} from "forge-std/console2.sol";
import {FormatLib} from "super-sol/libraries/FormatLib.sol";

/// @author philogy <https://github.com/philogy>
/// @dev Interacts with pools
contract PoolGate is IUnlockCallback, CommonBase {
    using FormatLib for *;
    using SignedUnsignedLib for *;
    using PoolIdLibrary for PoolKey;
    using IUniV4 for IPoolManager;
    using ConversionLib for address;

    IPoolManager internal immutable UNI_V4;
    address public hook;

    int24 internal _tickSpacing = 60;

    constructor(address uniV4) {
        UNI_V4 = IPoolManager(uniV4);
    }

    function setHook(address hook_) external {
        hook = hook_;
    }

    function tickSpacing(int24 spacing) external {
        _tickSpacing = spacing;
    }

    function initializePool(
        address asset0,
        address asset1,
        uint160 initialSqrtPriceX96,
        uint16 storeIndex
    ) public returns (PoolId) {
        bytes memory data = UNI_V4.unlock(
            abi.encodeCall(this.__initializePool, (asset0, asset1, initialSqrtPriceX96, storeIndex))
        );
        return abi.decode(data, (PoolId));
    }

    function swap(
        address assetIn,
        address assetOut,
        int256 amountSpecified,
        uint160 sqrtPriceLimitX96
    ) public returns (BalanceDelta delta) {
        bytes memory data = UNI_V4.unlock(
            abi.encodeCall(this.__swap, (assetIn, assetOut, amountSpecified, sqrtPriceLimitX96))
        );
        delta = abi.decode(data, (BalanceDelta));
    }

    function addLiquidity(
        address asset0,
        address asset1,
        int24 tickLower,
        int24 tickUpper,
        uint256 liquidity,
        bytes32 salt
    ) public returns (uint256 amount0, uint256 amount1) {
        IPoolManager.ModifyLiquidityParams memory params = IPoolManager.ModifyLiquidityParams({
            tickLower: tickLower,
            tickUpper: tickUpper,
            liquidityDelta: liquidity.signed(),
            salt: salt
        });
        try UNI_V4.unlock(abi.encodeCall(this.__addLiquidity, (asset0, asset1, msg.sender, params)))
        returns (bytes memory data) {
            BalanceDelta delta = abi.decode(data, (BalanceDelta));
            amount0 = uint128(-delta.amount0());
            amount1 = uint128(-delta.amount1());
        } catch (bytes memory err) {
            console.log("stuff failed???");
            console.logBytes(err);
        }
    }

    function removeLiquidity(
        address asset0,
        address asset1,
        int24 tickLower,
        int24 tickUpper,
        uint256 liquidity,
        bytes32 salt
    ) public returns (uint256 amount0, uint256 amount1) {
        IPoolManager.ModifyLiquidityParams memory params = IPoolManager.ModifyLiquidityParams({
            tickLower: tickLower,
            tickUpper: tickUpper,
            liquidityDelta: liquidity.neg(),
            salt: salt
        });
        bytes memory data = UNI_V4.unlock(
            abi.encodeCall(this.__removeLiquidity, (asset0, asset1, msg.sender, params))
        );
        BalanceDelta delta = abi.decode(data, (BalanceDelta));
        amount0 = uint128(delta.amount0());
        amount1 = uint128(delta.amount1());
    }

    function mint(address asset, uint256 amount) public {
        mint(msg.sender, asset, amount);
    }

    function mint(address to, address asset, uint256 amount) public {
        UNI_V4.unlock(abi.encodeCall(this.__mint, (to, asset, amount)));
    }

    function isInitialized(address asset0, address asset1) public view returns (bool) {
        PoolKey memory poolKey = hook.toPoolKey(asset0, asset1, _tickSpacing);
        Slot0 slot0 = UNI_V4.getSlot0(poolKey.toId());
        return slot0.sqrtPriceX96() != 0;
    }

    function unlockCallback(bytes calldata data) external returns (bytes memory) {
        (bool success, bytes memory retData) = address(this).call(data);
        assembly ("memory-safe") {
            if iszero(success) { revert(add(retData, 0x20), mload(retData)) }
        }
        return retData;
    }

    function __swap(
        address assetIn,
        address assetOut,
        int256 amountSpecified,
        uint160 sqrtPriceLimitX96
    ) public returns (BalanceDelta swapDelta) {
        bool zeroForOne = assetIn < assetOut;
        PoolKey memory key = zeroForOne
            ? hook.toPoolKey(assetIn, assetOut, _tickSpacing)
            : hook.toPoolKey(assetOut, assetIn, _tickSpacing);
        swapDelta = UNI_V4.swap(
            key, IPoolManager.SwapParams(zeroForOne, amountSpecified, sqrtPriceLimitX96), ""
        );
        _clearDelta(Currency.unwrap(key.currency0), swapDelta.amount0());
        _clearDelta(Currency.unwrap(key.currency1), swapDelta.amount1());
    }

    function __initializePool(
        address asset0,
        address asset1,
        uint160 initialSqrtPriceX96,
        uint16 storeIndex
    ) public returns (PoolId) {
        PoolKey memory poolKey = hook.toPoolKey(asset0, asset1, _tickSpacing);
        UNI_V4.initialize(poolKey, initialSqrtPriceX96, bytes.concat(bytes2(storeIndex)));
        return PoolIdLibrary.toId(poolKey);
    }

    function __addLiquidity(
        address asset0,
        address asset1,
        address sender,
        IPoolManager.ModifyLiquidityParams calldata params
    ) public returns (BalanceDelta callerDelta) {
        PoolKey memory poolKey = hook.toPoolKey(asset0, asset1, _tickSpacing);
        if (address(vm).code.length > 0) vm.startPrank(sender);
        BalanceDelta feeDelta;
        (callerDelta, feeDelta) = UNI_V4.modifyLiquidity(poolKey, params, "");
        require(feeDelta.amount0() == 0 && feeDelta.amount1() == 0, "Getting fees?");
        require(
            callerDelta.amount0() <= 0 && callerDelta.amount1() <= 0,
            "getting tokens for adding liquidity"
        );
        _clear(asset0, asset1, callerDelta);
        if (address(vm).code.length > 0) vm.stopPrank();
    }

    function __removeLiquidity(
        address asset0,
        address asset1,
        address sender,
        IPoolManager.ModifyLiquidityParams calldata params
    ) public returns (BalanceDelta delta) {
        PoolKey memory poolKey = hook.toPoolKey(asset0, asset1, _tickSpacing);
        if (address(vm).code.length > 0) vm.startPrank(sender);
        (delta,) = UNI_V4.modifyLiquidity(poolKey, params, "");

        bytes32 delta0Slot = keccak256(abi.encode(sender, asset0));
        bytes32 delta1Slot = keccak256(abi.encode(sender, asset1));
        bytes32 rawDelta0 = UNI_V4.exttload(delta0Slot);
        bytes32 rawDelta1 = UNI_V4.exttload(delta1Slot);
        delta = delta
            + toBalanceDelta(int128(int256(uint256(rawDelta0))), int128(int256(uint256(rawDelta1))));

        require(delta.amount0() >= 0 && delta.amount1() >= 0, "losing money for removing liquidity");
        _clear(asset0, asset1, delta);

        if (address(vm).code.length > 0) vm.stopPrank();
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

    function _clear(address asset0, address asset1, BalanceDelta delta) internal {
        _clearDelta(asset0, delta.amount0());
        _clearDelta(asset1, delta.amount1());
    }

    function _clearDelta(address asset, int128 delta) internal {
        if (delta > 0) {
            UNI_V4.clear(asset.intoC(), uint128(delta));
        } else if (delta < 0) {
            unchecked {
                _settleMintable(asset, uint128(-delta), true);
            }
        }
    }
}
