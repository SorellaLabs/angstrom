// SPDX-License-Identifier: BUSL-1.1
pragma solidity ^0.8.0;

import {UniConsumer} from "./UniConsumer.sol";

import {IPoolManager} from "v4-core/src/interfaces/IPoolManager.sol";
import {DeltaTracker} from "../types/DeltaTracker.sol";
import {BalanceDelta} from "v4-core/src/types/BalanceDelta.sol";
import {AssetArray, Asset, FEE_SUMMARY_ENTRY_SIZE} from "../types/Asset.sol";
import {
    PriceAB as PriceOutVsIn, AmountA as AmountOut, AmountB as AmountIn
} from "../types/Price.sol";
import {CalldataReader} from "../types/CalldataReader.sol";
import {IUniV4} from "../interfaces/IUniV4.sol";
import {PoolKey} from "v4-core/src/types/PoolKey.sol";
import {PoolId, PoolIdLibrary} from "v4-core/src/types/PoolId.sol";
import {Currency} from "v4-core/src/types/Currency.sol";

import {SafeTransferLib} from "solady/src/utils/SafeTransferLib.sol";
import {ConversionLib} from "src/libraries/ConversionLib.sol";

/// @author philogy <https://github.com/philogy>
abstract contract SettlementManager is UniConsumer {
    using IUniV4 for IPoolManager;
    using SafeTransferLib for address;
    using ConversionLib for address;

    error BundleChangeNetNegative(address asset);

    DeltaTracker internal bundleDeltas;

    mapping(address => mapping(address => uint256)) internal _angstromReserves;

    function _takeAssets(AssetArray assets) internal {
        uint256 length = assets.len();
        for (uint256 i = 0; i < length; i++) {
            Asset asset = assets.getUnchecked(i);
            uint256 amount = asset.take();
            if (amount > 0) {
                address addr = asset.addr();
                UNI_V4.take(addr.intoC(), address(this), amount);
                bundleDeltas.add(addr, amount);
            }
        }
    }

    function _saveAndSettle(AssetArray assets) internal {
        uint256 length = assets.len();

        // Allocate fee summary buffer.
        uint256 raw_feeSummaryStartPtr;
        assembly ("memory-safe") {
            raw_feeSummaryStartPtr := mload(0x40)
            mstore(0x40, add(raw_feeSummaryStartPtr, mul(length, FEE_SUMMARY_ENTRY_SIZE)))
        }
        uint256 raw_feeSummaryPtr = raw_feeSummaryStartPtr;

        for (uint256 i = 0; i < length; i++) {
            Asset asset = assets.getUnchecked(i);
            address addr = asset.addr();
            uint256 saving = asset.save();
            uint256 settle = asset.settle();

            if (bundleDeltas.sub(addr, saving + settle) < 0) {
                revert BundleChangeNetNegative(addr);
            }

            if (settle > 0) {
                UNI_V4.sync(addr.intoC());
                addr.safeTransfer(address(UNI_V4), settle);
                UNI_V4.settle();
            }

            asset.raw_copyFeeEntryToMemory(raw_feeSummaryPtr);
            unchecked {
                raw_feeSummaryPtr += FEE_SUMMARY_ENTRY_SIZE;
            }
        }

        // Hash buffer and emit unique log.
        assembly ("memory-safe") {
            mstore(0x00, keccak256(raw_feeSummaryStartPtr, mul(length, FEE_SUMMARY_ENTRY_SIZE)))
            log0(0x00, 0x20)
        }
    }

    /// @dev Sends rewards by crediting them delta in the pool manager. WARN: expects invoker to
    /// validate accounting for `amount`.
    function _settleRewardViaUniswapTo(address to, Currency asset, uint256 amount) internal {
        if (amount == 0) return;
        UNI_V4.sync(asset);
        Currency.unwrap(asset).safeTransfer(address(UNI_V4), amount);
        UNI_V4.settleFor(to);
    }

    function _settleOrderIn(address from, address asset, AmountIn amountIn, bool useInternal)
        internal
    {
        uint256 amount = amountIn.into();
        bundleDeltas.add(asset, amount);
        if (useInternal) {
            _angstromReserves[from][asset] -= amount;
        } else {
            asset.safeTransferFrom(from, address(this), amount);
        }
    }

    function _settleOrderOut(address to, address asset, AmountOut amountOut, bool useInternal)
        internal
    {
        uint256 amount = amountOut.into();
        bundleDeltas.sub(asset, amount);
        if (useInternal) _angstromReserves[to][asset] += amount;
        else asset.safeTransfer(to, amount);
    }
}
