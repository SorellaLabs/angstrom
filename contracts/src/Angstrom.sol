// SPDX-License-Identifier: BUSL-1.1
pragma solidity 0.8.26;

import {ERC712} from "./modules/ERC712.sol";
import {NodeManager} from "./modules/NodeManager.sol";
import {SettlementManager} from "./modules/SettlementManager.sol";
import {PoolUpdateManager} from "./modules/PoolUpdateManager.sol";
import {InvalidationManager} from "./modules/InvalidationManager.sol";
import {HookManager} from "./modules/HookManager.sol";
import {IPoolManager} from "v4-core/src/interfaces/IPoolManager.sol";
import {UniConsumer} from "./modules/UniConsumer.sol";
import {IUnlockCallback} from "v4-core/src/interfaces/callback/IUnlockCallback.sol";

import {TypedDataHasher} from "./types/TypedDataHasher.sol";

import {AssetArray, AssetLib} from "./types/Asset.sol";
import {PairArray, Pair, PairLib} from "./types/Pair.sol";
import {ToBOrderBuffer} from "./types/ToBOrderBuffer.sol";
import {UserOrderBuffer} from "./types/UserOrderBuffer.sol";
import {UserOrderVariantMap} from "./types/UserOrderVariantMap.sol";
import {ToBOrderVariantMap} from "./types/ToBOrderVariantMap.sol";
import {HookBuffer, HookBufferLib} from "./types/HookBuffer.sol";
import {CalldataReader, CalldataReaderLib} from "./types/CalldataReader.sol";
import {SignatureLib} from "./libraries/SignatureLib.sol";
import {
    PriceAB as PriceOutVsIn, AmountA as AmountOut, AmountB as AmountIn
} from "./types/Price.sol";
import {PoolConfigStore} from "./libraries/pool-config/PoolConfigStore.sol";

import {RayMathLib} from "./libraries/RayMathLib.sol";

import {console} from "forge-std/console.sol";
// TODO: Remove
import {FormatLib} from "super-sol/libraries/FormatLib.sol";

/// @author philogy <https://github.com/philogy>
contract Angstrom is
    ERC712,
    InvalidationManager,
    SettlementManager,
    NodeManager,
    HookManager,
    PoolUpdateManager,
    IUnlockCallback
{
    using RayMathLib for uint256;
    // TODO: Remove
    using FormatLib for *;

    error LimitViolated();

    constructor(IPoolManager uniV4, address controller, address feeMaster)
        UniConsumer(uniV4)
        NodeManager(controller)
        SettlementManager(feeMaster)
    {}

    function execute(bytes calldata encoded) external {
        _nodeBundleLock();
        UNI_V4.unlock(encoded);
    }

    function unlockCallback(bytes calldata data)
        external
        override
        onlyUniV4
        returns (bytes memory)
    {
        CalldataReader reader = CalldataReaderLib.from(data);

        AssetArray assets;
        (reader, assets) = AssetLib.readFromAndValidate(reader);
        PairArray pairs;
        (reader, pairs) = PairLib.readFromAndValidate(reader, assets, _configStore);

        _takeAssets(assets);
        reader = _updatePools(reader, bundleDeltas, pairs);
        reader = _validateAndExecuteToBOrders(reader, pairs);
        reader = _validateAndExecuteUserOrders(reader, pairs);
        reader.requireAtEndOf(data);

        _saveAndSettle(assets);

        return new bytes(0);
    }

    function extsload(bytes32 slot) external view returns (bytes32) {
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
        // No ERC712 variants for ToB orders so typehash can remain unchanged.
        buffer.setTypeHash();

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
        ToBOrderVariantMap variantMap;
        {
            uint8 variantByte;
            (reader, variantByte) = reader.readU8();
            variantMap = ToBOrderVariantMap.wrap(variantByte);
        }

        buffer.useInternal = variantMap.useInternal();

        (reader, buffer.quantityIn) = reader.readU128();
        (reader, buffer.quantityOut) = reader.readU128();

        {
            uint16 pairIndex;
            (reader, pairIndex) = reader.readU16();
            (buffer.assetIn, buffer.assetOut) =
                pairs.get(pairIndex).getAssets(variantMap.zeroForOne());
        }

        (reader, buffer.recipient) =
            variantMap.recipientIsSome() ? reader.readAddr() : (reader, address(0));

        HookBuffer hook;
        (reader, hook, buffer.hookDataHash) = HookBufferLib.readFrom(reader, variantMap.noHook());

        // The `.hash` method validates the `block.number` for flash orders.
        bytes32 orderHash = typedHasher.hashTypedData(buffer.hash());

        _invalidateOrderHash(orderHash);

        address from;
        (reader, from) = variantMap.isEcdsa()
            ? SignatureLib.readAndCheckEcdsa(reader, orderHash)
            : SignatureLib.readAndCheckERC1271(reader, orderHash);

        hook.tryTrigger(from);

        _settleOrderIn(
            from, buffer.assetIn, AmountIn.wrap(buffer.quantityIn), variantMap.useInternal()
        );
        address to = _defaultOr(buffer.recipient, from);
        _settleOrderOut(
            to, buffer.assetOut, AmountOut.wrap(buffer.quantityOut), variantMap.useInternal()
        );
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
        {
            uint8 variantByte;
            (reader, variantByte) = reader.readU8();
            variantMap = UserOrderVariantMap.wrap(variantByte);
        }

        buffer.setTypeHash(variantMap);
        buffer.useInternal = variantMap.useInternal();

        // Load and lookup asset in/out and dependent values.
        PriceOutVsIn price;
        uint256 feeInE6;
        {
            uint256 priceOutVsIn;
            uint16 pairIndex;
            (reader, pairIndex) = reader.readU16();
            (buffer.assetIn, buffer.assetOut, priceOutVsIn, feeInE6) =
                pairs.get(pairIndex).getSwapInfo(variantMap.aToB());
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
        (reader, amountIn, amountOut) =
            buffer.loadAndComputeQuantity(reader, variantMap, price, feeInE6);

        bytes32 orderHash = buffer.hash712(variantMap, typedHasher);

        address from;
        (reader, from) = variantMap.isEcdsa()
            ? SignatureLib.readAndCheckEcdsa(reader, orderHash)
            : SignatureLib.readAndCheckERC1271(reader, orderHash);

        if (variantMap.isStanding()) {
            _checkDeadline(buffer.deadline_or_empty);
            _invalidateNonce(from, buffer.nonce_or_validForBlock);
        } else {
            _invalidateOrderHash(orderHash);
        }

        hook.tryTrigger(from);

        _settleOrderIn(from, buffer.assetIn, amountIn, variantMap.useInternal());
        address to = _defaultOr(buffer.recipient, from);
        _settleOrderOut(to, buffer.assetOut, amountOut, variantMap.useInternal());

        return reader;
    }

    function _defaultOr(address defaultAddr, address alt) internal pure returns (address addr) {
        assembly {
            addr := xor(shr(defaultAddr, alt), defaultAddr)
        }
    }
}
