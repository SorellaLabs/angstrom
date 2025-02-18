// SPDX-License-Identifier: BUSL-1.1
pragma solidity =0.8.26;

import {console} from "forge-std/console.sol";
import {EIP712} from "solady/src/utils/EIP712.sol";
import {TopLevelAuth} from "./modules/TopLevelAuth.sol";
import {Settlement} from "./modules/Settlement.sol";
import {PoolUpdates} from "./modules/PoolUpdates.sol";
import {UnlockHook} from "./modules/UnlockHook.sol";
import {OrderInvalidation} from "./modules/OrderInvalidation.sol";
import {IPoolManager} from "v4-core/src/interfaces/IPoolManager.sol";
import {UniConsumer} from "./modules/UniConsumer.sol";
import {PermitSubmitterHook} from "./modules/PermitSubmitterHook.sol";
import {IUnlockCallback} from "v4-core/src/interfaces/callback/IUnlockCallback.sol";

import {CalldataReader, CalldataReaderLib} from "./types/CalldataReader.sol";
import {AssetArray, AssetLib} from "./types/Asset.sol";
import {PairArray, PairLib} from "./types/Pair.sol";
import {TypedDataHasher, TypedDataHasherLib} from "./types/TypedDataHasher.sol";
import {HookBuffer, HookBufferLib} from "./types/HookBuffer.sol";
import {SignatureLib} from "./libraries/SignatureLib.sol";
import {
    PriceAB as PriceOutVsIn, AmountA as AmountOut, AmountB as AmountIn
} from "./types/Price.sol";
import {ToBOrderBuffer} from "./types/ToBOrderBuffer.sol";
import {ToBOrderVariantMap} from "./types/ToBOrderVariantMap.sol";
import {UserOrderBuffer} from "./types/UserOrderBuffer.sol";
import {UserOrderVariantMap} from "./types/UserOrderVariantMap.sol";
import {TWAPOrderBuffer} from "./types/TWAPOrderBuffer.sol";
import {TWAPOrderVariantMap} from "./types/TWAPOrderVariantMap.sol";

/// @author philogy <https://github.com/philogy>
contract Angstrom is
    EIP712,
    OrderInvalidation,
    Settlement,
    PoolUpdates,
    UnlockHook,
    IUnlockCallback,
    PermitSubmitterHook
{
    error LimitViolated();
    error ToBGasUsedAboveMax();

    constructor(IPoolManager uniV4, address controller)
        UniConsumer(uniV4)
        TopLevelAuth(controller)
    {
        _checkAngstromHookFlags();
    }

    function execute(bytes calldata encoded) external {
        console.log("testing we got here");
        _nodeBundleLock();
        if (encoded.length > 0) {
            UNI_V4.unlock(encoded);
        }
    }

    function unlockCallback(bytes calldata data) external override returns (bytes memory) {
        _onlyUniV4();

        CalldataReader reader = CalldataReaderLib.from(data);

        AssetArray assets;
        (reader, assets) = AssetLib.readFromAndValidate(reader);
        PairArray pairs;
        (reader, pairs) = PairLib.readFromAndValidate(reader, assets, _configStore);

        console.log("read pairs and assets");
        _takeAssets(assets);
        console.log("took assets");

        reader = _updatePools(reader, pairs);
        console.log("updated pools");
        reader = _validateAndExecuteToBOrders(reader, pairs);
        console.log("executed tob");
        reader = _validateAndExecuteUserOrders(reader, pairs);
        console.log("executed user");
        reader = _validateAndExecuteTWAPOrders(reader, pairs);
        console.log("executed twap");
        reader.requireAtEndOf(data);
        _saveAndSettle(assets);

        // Return empty bytes.
        assembly ("memory-safe") {
            mstore(0x00, 0x20) // Dynamic type relative offset
            mstore(0x20, 0x00) // Bytes length
            return(0x00, 0x40)
        }
    }

    /// @dev Load arbitrary storage slot from this contract, enables on-chain introspection without
    /// view methods.
    function extsload(uint256 slot) external view returns (uint256) {
        assembly ("memory-safe") {
            mstore(0x00, sload(slot))
            return(0x00, 0x20)
        }
    }

    function _validateAndExecuteToBOrders(CalldataReader reader, PairArray pairs)
        internal
        returns (CalldataReader)
    {
        CalldataReader end;
        (reader, end) = reader.readU24End();

        TypedDataHasher typedHasher = _erc712Hasher();
        ToBOrderBuffer memory buffer;
        buffer.init();

        // Purposefully devolve into an endless loop if the specified length isn't exactly used s.t.
        // `reader == end` at some point.
        while (reader != end) {
            reader = _validateAndExecuteToBOrder(reader, buffer, typedHasher, pairs);
        }

        return reader;
    }

    function _validateAndExecuteToBOrder(
        CalldataReader reader,
        ToBOrderBuffer memory buffer,
        TypedDataHasher typedHasher,
        PairArray pairs
    ) internal returns (CalldataReader) {
        // Load `TopOfBlockOrder` PADE variant map which will inform later variable-type encoding.
        ToBOrderVariantMap variantMap;
        {
            uint8 variantByte;
            (reader, variantByte) = reader.readU8();
            variantMap = ToBOrderVariantMap.wrap(variantByte);
        }

        buffer.useInternal = variantMap.useInternal();

        (reader, buffer.quantityIn) = reader.readU128();
        (reader, buffer.quantityOut) = reader.readU128();
        (reader, buffer.maxGasAsset0) = reader.readU128();
        // Decode, validate & apply gas fee.
        uint128 gasUsedAsset0;
        (reader, gasUsedAsset0) = reader.readU128();
        if (gasUsedAsset0 > buffer.maxGasAsset0) revert ToBGasUsedAboveMax();

        {
            uint16 pairIndex;
            (reader, pairIndex) = reader.readU16();
            (buffer.assetIn, buffer.assetOut) =
                pairs.get(pairIndex).getAssets(variantMap.zeroForOne());
        }

        (reader, buffer.recipient) =
            variantMap.recipientIsSome() ? reader.readAddr() : (reader, address(0));

        bytes32 orderHash = typedHasher.hashTypedData(buffer.hash());

        address from;
        (reader, from) = variantMap.isEcdsa()
            ? SignatureLib.readAndCheckEcdsa(reader, orderHash)
            : SignatureLib.readAndCheckERC1271(reader, orderHash);

        _invalidateOrderHash(orderHash, from);
        console.log(from);

        address to = buffer.recipient;
        assembly ("memory-safe") {
            to := or(mul(iszero(to), from), to)
        }

        if (variantMap.zeroForOne()) {
            buffer.quantityIn += gasUsedAsset0;
        } else {
            buffer.quantityOut -= gasUsedAsset0;
        }
        _settleOrderIn(from, buffer.assetIn, AmountIn.wrap(buffer.quantityIn), buffer.useInternal);
        _settleOrderOut(to, buffer.assetOut, AmountOut.wrap(buffer.quantityOut), buffer.useInternal);

        return reader;
    }

    function _validateAndExecuteUserOrders(CalldataReader reader, PairArray pairs)
        internal
        returns (CalldataReader)
    {
        TypedDataHasher typedHasher = _erc712Hasher();
        UserOrderBuffer memory buffer;

        CalldataReader end;
        (reader, end) = reader.readU24End();

        // Purposefully devolve into an endless loop if the specified length isn't exactly used s.t.
        // `reader == end` at some point.
        while (reader != end) {
            reader = _validateAndExecuteUserOrder(reader, buffer, typedHasher, pairs);
        }

        return reader;
    }

    function _validateAndExecuteUserOrder(
        CalldataReader reader,
        UserOrderBuffer memory buffer,
        TypedDataHasher typedHasher,
        PairArray pairs
    ) internal returns (CalldataReader) {
        UserOrderVariantMap variantMap;
        // Load variant map, ref id and set use internal.
        (reader, variantMap) = buffer.init(reader);

        // Load and lookup asset in/out and dependent values.
        PriceOutVsIn price;
        {
            uint256 priceOutVsIn;
            uint16 pairIndex;
            (reader, pairIndex) = reader.readU16();
            (buffer.assetIn, buffer.assetOut, priceOutVsIn) =
                pairs.get(pairIndex).getSwapInfo(variantMap.zeroForOne());
            price = PriceOutVsIn.wrap(priceOutVsIn);
        }

        (reader, buffer.minPrice) = reader.readU256();
        if (price.into() < buffer.minPrice) revert LimitViolated();

        (reader, buffer.recipient) =
            variantMap.recipientIsSome() ? reader.readAddr() : (reader, address(0));

        HookBuffer hook;
        (reader, hook, buffer.hookDataHash) = HookBufferLib.readFrom(reader, variantMap.noHook());

        // For flash orders sets the current block number as `validForBlock` so that it's
        // implicitly validated via hashing later.
        reader = buffer.readOrderValidation(reader, variantMap);

        AmountIn amountIn;
        AmountOut amountOut;
        (reader, amountIn, amountOut) = buffer.loadAndComputeQuantity(reader, variantMap, price);

        bytes32 orderHash = typedHasher.hashTypedData(buffer.structHash(variantMap));

        address from;
        (reader, from) = variantMap.isEcdsa()
            ? SignatureLib.readAndCheckEcdsa(reader, orderHash)
            : SignatureLib.readAndCheckERC1271(reader, orderHash);

        if (variantMap.isStanding()) {
            _checkDeadline(buffer.deadline_or_empty);
            _invalidateNonce(from, buffer.nonce_or_validForBlock);
        } else {
            _invalidateOrderHash(orderHash, from);
        }

        // Push before hook as a potential loan.
        address to = buffer.recipient;
        assembly ("memory-safe") {
            to := or(mul(iszero(to), from), to)
        }
        _settleOrderOut(to, buffer.assetOut, amountOut, buffer.useInternal);

        hook.tryTrigger(from);

        _settleOrderIn(from, buffer.assetIn, amountIn, buffer.useInternal);

        return reader;
    }

    function _validateAndExecuteTWAPOrders(CalldataReader reader, PairArray pairs)
        internal
        returns (CalldataReader)
    {
        TypedDataHasher typedHasher = _erc712Hasher();
        TWAPOrderBuffer memory buffer;

        CalldataReader end;
        (reader, end) = reader.readU24End();

        // Purposefully devolve into an endless loop if the specified length isn't exactly used s.t.
        // `reader == end` at some point.
        while (reader != end) {
            reader = _validateAndExecuteTWAPOrder(reader, buffer, typedHasher, pairs);
        }

        return reader;
    }

    function _validateAndExecuteTWAPOrder(
        CalldataReader reader,
        TWAPOrderBuffer memory buffer,
        TypedDataHasher typedHasher,
        PairArray pairs
    ) internal returns (CalldataReader) {
        TWAPOrderVariantMap variantMap;
        // Load variant map, ref id and set use internal.
        (reader, variantMap) = buffer.init(reader);

        // Load and lookup asset in/out and dependent values.
        PriceOutVsIn price;
        {
            uint256 priceOutVsIn;
            uint16 pairIndex;
            (reader, pairIndex) = reader.readU16();
            (buffer.assetIn, buffer.assetOut, priceOutVsIn) =
                pairs.get(pairIndex).getSwapInfo(variantMap.zeroForOne());
            price = PriceOutVsIn.wrap(priceOutVsIn);
        }

        (reader, buffer.minPrice) = reader.readU256();
        if (price.into() < buffer.minPrice) revert LimitViolated();

        (reader, buffer.recipient) =
            variantMap.recipientIsSome() ? reader.readAddr() : (reader, address(0));

        HookBuffer hook;
        (reader, hook, buffer.hookDataHash) = HookBufferLib.readFrom(reader, variantMap.noHook());

        reader = buffer.readOrderValidation(reader);

        AmountIn amountIn;
        AmountOut amountOut;
        (reader, amountIn, amountOut) = buffer.loadAndComputeQuantity(reader, variantMap, price);

        address from;
        {
            bytes32 orderHash = typedHasher.hashTypedData(buffer.hash());
            (reader, from) = variantMap.isEcdsa()
                ? SignatureLib.readAndCheckEcdsa(reader, orderHash)
                : SignatureLib.readAndCheckERC1271(reader, orderHash);
        }

        _checkTWAPOrderData(buffer.timeInterval, buffer.totalParts, buffer.window);
        _invalidatePartTWAPNonceAndCheckDeadline(
            from, 
            buffer.nonce, 
            buffer.startTime, 
            buffer.timeInterval, 
            buffer.totalParts,
            buffer.window
        );

        // Push before hook as a potential loan.
        address to = buffer.recipient;
        assembly ("memory-safe") {
            to := or(mul(iszero(to), from), to)
        }
        _settleOrderOut(to, buffer.assetOut, amountOut, buffer.useInternal);

        hook.tryTrigger(from);

        _settleOrderIn(from, buffer.assetIn, amountIn, buffer.useInternal);

        return reader;
    }

    function _domainNameAndVersion()
        internal
        pure
        override
        returns (string memory, string memory)
    {
        return ("Angstrom", "v1");
    }

    function _erc712Hasher() internal view returns (TypedDataHasher) {
        return TypedDataHasherLib.init(_domainSeparator());
    }
}
