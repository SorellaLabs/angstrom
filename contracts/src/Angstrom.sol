// SPDX-License-Identifier: BUSL-1.1
pragma solidity 0.8.26;

import {ERC712} from "./modules/ERC712.sol";
import {NodeManager} from "./modules/NodeManager.sol";
import {SettlementManager} from "./modules/SettlementManager.sol";
import {PoolUpdateManager} from "./modules/PoolUpdateManager.sol";
import {InvalidationManager} from "./modules/InvalidationManager.sol";
import {HookManager} from "./modules/HookManager.sol";
import {UniConsumer} from "./modules/UniConsumer.sol";
import {IUnlockCallback} from "v4-core/src/interfaces/callback/IUnlockCallback.sol";

import {TypedDataHasher} from "./types/TypedDataHasher.sol";

import {AssetArray, AssetLib} from "./types/Asset.sol";
import {PairArray, Pair, PairLib} from "./types/Pair.sol";
import {ToBOrderBuffer} from "./types/ToBOrderBuffer.sol";
import {UserOrderBuffer} from "./types/UserOrderBuffer.sol";
import {OrderVariantMap} from "./types/OrderVariantMap.sol";
import {HookBuffer, HookBufferLib} from "./types/HookBuffer.sol";
import {CalldataReader, CalldataReaderLib} from "./types/CalldataReader.sol";
import {SignatureLib} from "./libraries/SignatureLib.sol";
import {PriceAB as PriceOutVsIn, AmountA as AmountOut, AmountB as AmountIn} from "./types/Price.sol";

import {RayMathLib} from "./libraries/RayMathLib.sol";

import {console} from "forge-std/console.sol";
import {DEBUG_LOGS} from "./modules/DevFlags.sol";
// TODO: Remove
import {FormatLib} from "super-sol/libraries/FormatLib.sol";

/// @author philogy <https://github.com/philogy>
contract Angstrom is
    ERC712,
    InvalidationManager,
    PoolUpdateManager,
    SettlementManager,
    NodeManager,
    HookManager,
    IUnlockCallback
{
    using RayMathLib for uint256;
    // TODO: Remove
    using FormatLib for *;

    error LimitViolated();

    constructor(address uniV4PoolManager, address governance) UniConsumer(uniV4PoolManager) NodeManager(governance) {}

    function execute(bytes calldata encoded) external {
        _nodeBundleLock();
        UNI_V4.unlock(encoded);
    }

    function unlockCallback(bytes calldata data) external override onlyUniV4 returns (bytes memory) {
        CalldataReader reader = CalldataReaderLib.from(data);

        AssetArray assets;
        (reader, assets) = AssetLib.readFromAndValidate(reader);
        PairArray pairs;
        (reader, pairs) = PairLib.readFromAndValidate(reader);

        _takeAssets(assets);
        reader = _updatePools(reader, tBundleDeltas, assets);
        reader = _validateAndExecuteToBOrders(reader, assets);
        reader = _validateAndExecuteUserOrders(reader, assets, pairs);
        _saveAndSettle(assets);

        reader.requireAtEndOf(data);

        return new bytes(0);
    }

    function _validateAndExecuteToBOrders(CalldataReader reader, AssetArray assets) internal returns (CalldataReader) {
        CalldataReader end;
        (reader, end) = reader.readU24End();

        TypedDataHasher typedHasher = _erc712Hasher();
        ToBOrderBuffer memory buffer;
        // No ERC712 variants for ToB orders so typehash can remain unchanged.
        buffer.setTypeHash();

        // Purposefully devolve into an endless loop if the specified length isn't exactly used s.t.
        // `reader == end` at some point.
        while (reader != end) {
            reader = _validateAndExecuteToBOrder(reader, buffer, typedHasher, assets);
        }

        return reader;
    }

    function _validateAndExecuteToBOrder(
        CalldataReader reader,
        ToBOrderBuffer memory buffer,
        TypedDataHasher typedHasher,
        AssetArray assets
    ) internal returns (CalldataReader) {
        OrderVariantMap variant;
        (reader, variant) = reader.readVariant();

        buffer.useInternal = variant.useInternal();

        (reader, buffer.quantityIn) = reader.readU128();
        (reader, buffer.quantityOut) = reader.readU128();

        {
            uint16 indexA;
            (reader, indexA) = reader.readU16();
            buffer.assetIn = assets.get(indexA).addr();
            uint16 indexB;
            (reader, indexB) = reader.readU16();
            buffer.assetOut = assets.get(indexB).addr();
        }

        (reader, buffer.recipient) = variant.recipientIsSome() ? reader.readAddr() : (reader, address(0));

        HookBuffer hook;
        (reader, hook, buffer.hookDataHash) = HookBufferLib.readFrom(reader, variant.noHook());

        // The `.hash` method validates the `block.number` for flash orders.
        bytes32 orderHash = typedHasher.hashTypedData(buffer.hash());

        _invalidateOrderHash(orderHash);

        address from;
        (reader, from) = variant.isEcdsa()
            ? SignatureLib.readAndCheckEcdsa(reader, orderHash)
            : SignatureLib.readAndCheckERC1271(reader, orderHash);

        hook.tryTrigger(from);

        _settleOrderIn(from, buffer.assetIn, AmountIn.wrap(buffer.quantityIn), variant.useInternal());
        address to = _defaultOr(buffer.recipient, from);
        _settleOrderOut(to, buffer.assetOut, AmountOut.wrap(buffer.quantityOut), variant.useInternal());
        return reader;
    }

    uint256 debug_orderCounter;

    function _validateAndExecuteUserOrders(CalldataReader reader, AssetArray assets, PairArray pairs)
        internal
        returns (CalldataReader)
    {
        TypedDataHasher typedHasher = _erc712Hasher();
        UserOrderBuffer memory buffer;

        CalldataReader end;
        (reader, end) = reader.readU24End();

        if (DEBUG_LOGS) debug_orderCounter = 0;

        // Purposefully devolve into an endless loop if the specified length isn't exactly used s.t.
        // `reader == end` at some point.
        while (reader != end) {
            if (DEBUG_LOGS) console.log("[%s]", debug_orderCounter++);
            reader = _validateAndExecuteUserOrder(reader, buffer, typedHasher, assets, pairs);
        }

        return reader;
    }

    function _validateAndExecuteUserOrder(
        CalldataReader reader,
        UserOrderBuffer memory buffer,
        TypedDataHasher typedHasher,
        AssetArray assets,
        PairArray pairs
    ) internal returns (CalldataReader) {
        OrderVariantMap variant;
        (reader, variant) = reader.readVariant();

        buffer.setTypeHash(variant);
        buffer.useInternal = variant.useInternal();

        // Load and lookup asset in/out and dependent values.
        PriceOutVsIn price;
        {
            uint256 priceOutVsIn;
            (reader, buffer.assetIn, buffer.assetOut, priceOutVsIn) =
                pairs.decodeAndLookupPair(reader, assets, variant.aToB());
            price = PriceOutVsIn.wrap(priceOutVsIn);
        }

        (reader, buffer.minPrice) = reader.readU256();
        if (price.into() < buffer.minPrice) revert LimitViolated();

        (reader, buffer.recipient) = variant.recipientIsSome() ? reader.readAddr() : (reader, address(0));

        HookBuffer hook;
        (reader, hook, buffer.hookDataHash) = HookBufferLib.readFrom(reader, variant.noHook());

        // For flash orders sets the current block number as `validForBlock` so that it's
        // implicitly validated via hashing later.
        reader = buffer.readOrderValidation(reader, variant);

        AmountIn amountIn;
        AmountOut amountOut;
        (reader, amountIn, amountOut) = buffer.loadAndComputeQuantity(reader, variant, price, halfSpreadRay);

        if (DEBUG_LOGS) buffer.logBytes(variant);

        bytes32 orderHash = buffer.hash712(variant, typedHasher);

        address from;
        (reader, from) = variant.isEcdsa()
            ? SignatureLib.readAndCheckEcdsa(reader, orderHash)
            : SignatureLib.readAndCheckERC1271(reader, orderHash);

        if (variant.isStanding()) {
            _checkDeadline(buffer.deadline_or_empty);
            _invalidateNonce(from, buffer.nonce_or_validForBlock);
        } else {
            _invalidateOrderHash(orderHash);
        }

        hook.tryTrigger(from);

        _settleOrderIn(from, buffer.assetIn, amountIn, variant.useInternal());
        address to = _defaultOr(buffer.recipient, from);
        _settleOrderOut(to, buffer.assetOut, amountOut, variant.useInternal());

        return reader;
    }

    function _defaultOr(address defaultAddr, address alt) internal pure returns (address addr) {
        assembly {
            addr := xor(shr(defaultAddr, alt), defaultAddr)
        }
    }
}
