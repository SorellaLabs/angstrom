// SPDX-License-Identifier: BUSL-1.1
pragma solidity ^0.8.0;

import {IPoolManager} from "v4-core/src/interfaces/IPoolManager.sol";
import {tuint256} from "transient-goodies/TransientPrimitives.sol";
import {BalanceDelta} from "v4-core/src/types/BalanceDelta.sol";
import {PoolSwap, PoolSwapLib} from "../types/PoolSwap.sol";
import {AssetArray, Asset} from "../types/Asset.sol";
import {UniConsumer} from "./UniConsumer.sol";
import {PriceAB as PriceOutVsIn, AmountA as AmountOut, AmountB as AmountIn} from "../types/Price.sol";
import {CalldataReader} from "../types/CalldataReader.sol";

import {SafeTransferLib} from "solady/src/utils/SafeTransferLib.sol";
import {SignedUnsignedLib} from "super-sol/libraries/SignedUnsignedLib.sol";
import {ConversionLib} from "src/libraries/ConversionLib.sol";

import {console2 as console} from "forge-std/console2.sol";
import {FormatLib} from "super-sol/libraries/FormatLib.sol";

/// @author philogy <https://github.com/philogy>
abstract contract Accounter is UniConsumer {
    using SafeTransferLib for address;
    using ConversionLib for *;
    using SignedUnsignedLib for *;

    // TODO: Remove
    using FormatLib for *;

    /// @dev Uniswap's `MIN_SQRT_RATIO + 1` to pass the limit check.
    uint160 internal constant MIN_SQRT_RATIO = 4295128740;
    /// @dev Uniswap's `MAX_SQRT_RATIO - 1` to pass the limit check.
    uint160 internal constant MAX_SQRT_RATIO = 1461446703485210103287273052203988822378723970341;

    mapping(address => uint256) internal savedFees;
    mapping(address => tuint256) internal freeBalance;

    mapping(address => mapping(address => uint256)) internal _angstromReserves;

    function _borrowAssets(AssetArray assets) internal {
        uint256 length = assets.len();
        for (uint256 i = 0; i < length; i++) {
            Asset asset = assets.get(i);
            uint256 amount = asset.borrow();
            address addr = asset.addr();
            UNI_V4.take(addr.intoC(), address(this), amount);
            freeBalance[addr].inc(amount);
        }
    }

    function _execPoolSwaps(CalldataReader reader, AssetArray assets) internal returns (CalldataReader) {
        CalldataReader end;
        (reader, end) = reader.readU24End();

        while (reader != end) {
            PoolSwap swap;
            (reader, swap) = PoolSwapLib.readNextFrom(reader);
            (address asset0, address asset1, bool zeroForOne) = swap.getSwapAssets(assets);
            BalanceDelta delta = UNI_V4.swap(
                address(this).toPoolKey(asset0, asset1),
                IPoolManager.SwapParams({
                    zeroForOne: zeroForOne,
                    amountSpecified: swap.amountIn().neg(),
                    sqrtPriceLimitX96: zeroForOne ? MIN_SQRT_RATIO : MAX_SQRT_RATIO
                }),
                ""
            );
            freeBalance[asset0].dec(delta.amount0());
            freeBalance[asset1].dec(delta.amount1());
        }

        return reader;
    }

    function _saveAndSettle(AssetArray assets) internal {
        uint256 length = assets.len();
        for (uint256 i = 0; i < length; i++) {
            Asset asset = assets.get(i);
            address addr = asset.addr();
            uint256 saving = asset.save();
            uint256 settle = asset.settle();

            freeBalance[addr].dec(saving + settle);
            savedFees[addr] += saving;
            if (settle > 0) {
                UNI_V4.sync(addr.intoC());
                addr.safeTransfer(address(UNI_V4), settle);
                UNI_V4.settle();
            }
        }
    }

    function _accountIn(address from, address asset, AmountIn amountIn, bool useInternal) internal {
        uint256 amount = amountIn.into();
        freeBalance[asset].inc(amount);
        if (useInternal) _angstromReserves[from][asset] -= amount;
        else asset.safeTransferFrom(from, address(this), amount);
    }

    function _accountOut(address to, address asset, AmountOut amountOut, bool useInternal) internal {
        uint256 amount = amountOut.into();
        freeBalance[asset].dec(amount);
        if (useInternal) _angstromReserves[to][asset] += amount;
        else asset.safeTransfer(to, amount);
    }
}
