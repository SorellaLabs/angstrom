// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {UserOrderVariantMap} from "src/types/UserOrderVariantMap.sol";
import {OrderVariant as RefOrderVariant} from "./OrderVariant.sol";
import {UserOrderBufferLib} from "src/types/UserOrderBuffer.sol";
import {ToBOrderBufferLib} from "src/types/ToBOrderBuffer.sol";
import {SafeCastLib} from "solady/src/utils/SafeCastLib.sol";
import {Pair, PairLib} from "./Pair.sol";

import {FormatLib} from "super-sol/libraries/FormatLib.sol";
import {console} from "forge-std/console.sol";

struct OrderMeta {
    bool isEcdsa;
    address from;
    bytes signature;
}

/// @custom:erc712:exclude meta, amountFilled, gasUsedAsset0
struct PartialStandingOrder {
    uint32 refId;
    uint128 minAmountIn;
    uint128 maxAmountIn;
    uint128 maxGasAsset0;
    uint256 minPrice;
    bool useInternal;
    address assetIn;
    address assetOut;
    address recipient;
    address hook;
    bytes hookPayload;
    uint64 nonce;
    uint40 deadline;
    OrderMeta meta;
    uint128 amountFilled;
    uint128 gasUsedAsset0;
}

/// @custom:erc712:exclude meta, gasUsedAsset0
struct ExactStandingOrder {
    uint32 refId;
    bool exactIn;
    uint128 amount;
    uint128 maxGasAsset0;
    uint256 minPrice;
    bool useInternal;
    address assetIn;
    address assetOut;
    address recipient;
    address hook;
    bytes hookPayload;
    uint64 nonce;
    uint40 deadline;
    OrderMeta meta;
    uint128 gasUsedAsset0;
}

/// @custom:erc712:exclude meta, amountFilled, gasUsedAsset0
struct PartialFlashOrder {
    uint32 refId;
    uint128 minAmountIn;
    uint128 maxAmountIn;
    uint128 maxGasAsset0;
    uint256 minPrice;
    bool useInternal;
    address assetIn;
    address assetOut;
    address recipient;
    address hook;
    bytes hookPayload;
    uint64 validForBlock;
    OrderMeta meta;
    uint128 amountFilled;
    uint128 gasUsedAsset0;
}

/// @custom:erc712:exclude meta, gasUsedAsset0
struct ExactFlashOrder {
    uint32 refId;
    bool exactIn;
    uint128 amount;
    uint128 maxGasAsset0;
    uint256 minPrice;
    bool useInternal;
    address assetIn;
    address assetOut;
    address recipient;
    address hook;
    bytes hookPayload;
    uint64 validForBlock;
    OrderMeta meta;
    uint128 gasUsedAsset0;
}

/// @custom:erc712:exclude meta, gasUsedAsset0
/// @custom:another
struct TopOfBlockOrder {
    uint128 quantityIn;
    uint128 quantityOut;
    uint128 maxGasAsset0;
    bool useInternal;
    address assetIn;
    address assetOut;
    address recipient;
    uint64 validForBlock;
    OrderMeta meta;
    uint128 gasUsedAsset0;
}

using OrdersLib for OrderMeta global;
using OrdersLib for PartialStandingOrder global;
using OrdersLib for ExactStandingOrder global;
using OrdersLib for PartialFlashOrder global;
using OrdersLib for ExactFlashOrder global;
using OrdersLib for TopOfBlockOrder global;

library OrdersLib {
    using PairLib for *;
    using FormatLib for *;
    using SafeCastLib for *;

    function hash(PartialStandingOrder memory order) internal pure returns (bytes32) {
        return keccak256(
            abi.encode(
                UserOrderBufferLib.PARTIAL_STANDING_ORDER_TYPEHASH,
                order.refId,
                order.minAmountIn,
                order.maxAmountIn,
                order.maxGasAsset0,
                order.minPrice,
                order.useInternal,
                order.assetIn,
                order.assetOut,
                order.recipient,
                keccak256(_toHookData(order.hook, order.hookPayload)),
                order.nonce,
                order.deadline
            )
        );
    }

    function hash(ExactStandingOrder memory order) internal pure returns (bytes32) {
        return keccak256(
            abi.encode(
                UserOrderBufferLib.EXACT_STANDING_ORDER_TYPEHASH,
                order.refId,
                order.exactIn,
                order.amount,
                order.maxGasAsset0,
                order.minPrice,
                order.useInternal,
                order.assetIn,
                order.assetOut,
                order.recipient,
                keccak256(_toHookData(order.hook, order.hookPayload)),
                order.nonce,
                order.deadline
            )
        );
    }

    function hash(PartialFlashOrder memory order) internal pure returns (bytes32) {
        return keccak256(
            abi.encode(
                UserOrderBufferLib.PARTIAL_FLASH_ORDER_TYPEHASH,
                order.refId,
                order.minAmountIn,
                order.maxAmountIn,
                order.maxGasAsset0,
                order.minPrice,
                order.useInternal,
                order.assetIn,
                order.assetOut,
                order.recipient,
                keccak256(_toHookData(order.hook, order.hookPayload)),
                order.validForBlock
            )
        );
    }

    function hash(ExactFlashOrder memory order) internal pure returns (bytes32) {
        return keccak256(
            abi.encode(
                UserOrderBufferLib.EXACT_FLASH_ORDER_TYPEHASH,
                order.refId,
                order.exactIn,
                order.amount,
                order.maxGasAsset0,
                order.minPrice,
                order.useInternal,
                order.assetIn,
                order.assetOut,
                order.recipient,
                keccak256(_toHookData(order.hook, order.hookPayload)),
                order.validForBlock
            )
        );
    }

    function hash(TopOfBlockOrder memory order) internal pure returns (bytes32) {
        return keccak256(
            abi.encode(
                ToBOrderBufferLib.TOP_OF_BLOCK_ORDER_TYPEHASH,
                order.quantityIn,
                order.quantityOut,
                order.maxGasAsset0,
                order.useInternal,
                order.assetIn,
                order.assetOut,
                order.recipient,
                order.validForBlock
            )
        );
    }

    /// @dev WARNING: Assumes `pairs` are sorted.
    function encode(PartialStandingOrder memory order, Pair[] memory pairs)
        internal
        pure
        returns (bytes memory)
    {
        (uint16 pairIndex, bool zeroForOne) = pairs.getIndex(order.assetIn, order.assetOut);

        RefOrderVariant memory variantMap = RefOrderVariant({
            isExact: false,
            isFlash: false,
            isOut: false,
            noHook: order.hook == address(0),
            useInternal: order.useInternal,
            hasRecipient: order.recipient != address(0),
            isEcdsa: order.meta.isEcdsa,
            zeroForOne: zeroForOne
        });

        return bytes.concat(
            bytes.concat(
                bytes1(UserOrderVariantMap.unwrap(variantMap.encode())),
                bytes4(order.refId),
                bytes2(pairIndex),
                bytes32(order.minPrice),
                _encodeRecipient(order.recipient),
                _encodeHookData(order.hook, order.hookPayload),
                bytes8(order.nonce)
            ),
            bytes5(order.deadline),
            bytes16(order.minAmountIn),
            bytes16(order.maxAmountIn),
            bytes16(order.amountFilled),
            bytes16(order.maxGasAsset0),
            bytes16(order.gasUsedAsset0),
            _encodeSig(order.meta)
        );
    }

    function encode(ExactStandingOrder memory order, Pair[] memory pairs)
        internal
        pure
        returns (bytes memory)
    {
        (uint16 pairIndex, bool zeroForOne) = pairs.getIndex(order.assetIn, order.assetOut);

        RefOrderVariant memory variantMap = RefOrderVariant({
            isExact: true,
            isFlash: false,
            isOut: !order.exactIn,
            noHook: order.hook == address(0),
            useInternal: order.useInternal,
            hasRecipient: order.recipient != address(0),
            isEcdsa: order.meta.isEcdsa,
            zeroForOne: zeroForOne
        });

        return bytes.concat(
            bytes.concat(
                bytes1(UserOrderVariantMap.unwrap(variantMap.encode())),
                bytes4(order.refId),
                bytes2(pairIndex),
                bytes32(order.minPrice),
                _encodeRecipient(order.recipient),
                _encodeHookData(order.hook, order.hookPayload),
                bytes8(order.nonce)
            ),
            bytes5(order.deadline),
            bytes16(order.amount),
            bytes16(order.maxGasAsset0),
            bytes16(order.gasUsedAsset0),
            _encodeSig(order.meta)
        );
    }

    function encode(PartialFlashOrder memory order, Pair[] memory pairs)
        internal
        pure
        returns (bytes memory)
    {
        (uint16 pairIndex, bool zeroForOne) = pairs.getIndex(order.assetIn, order.assetOut);

        RefOrderVariant memory variantMap = RefOrderVariant({
            isExact: false,
            isFlash: true,
            isOut: false,
            noHook: order.hook == address(0),
            useInternal: order.useInternal,
            hasRecipient: order.recipient != address(0),
            isEcdsa: order.meta.isEcdsa,
            zeroForOne: zeroForOne
        });

        return bytes.concat(
            bytes.concat(
                bytes1(UserOrderVariantMap.unwrap(variantMap.encode())),
                bytes4(order.refId),
                bytes2(pairIndex),
                bytes32(order.minPrice),
                _encodeRecipient(order.recipient),
                _encodeHookData(order.hook, order.hookPayload),
                bytes16(order.minAmountIn)
            ),
            bytes16(order.maxAmountIn),
            bytes16(order.amountFilled),
            bytes16(order.maxGasAsset0),
            bytes16(order.gasUsedAsset0),
            _encodeSig(order.meta)
        );
    }

    function encode(ExactFlashOrder memory order, Pair[] memory pairs)
        internal
        pure
        returns (bytes memory)
    {
        (uint16 pairIndex, bool zeroForOne) = pairs.getIndex(order.assetIn, order.assetOut);

        RefOrderVariant memory variantMap = RefOrderVariant({
            isExact: true,
            isFlash: true,
            isOut: !order.exactIn,
            noHook: order.hook == address(0),
            useInternal: order.useInternal,
            hasRecipient: order.recipient != address(0),
            isEcdsa: order.meta.isEcdsa,
            zeroForOne: zeroForOne
        });

        return bytes.concat(
            bytes1(UserOrderVariantMap.unwrap(variantMap.encode())),
            bytes4(order.refId),
            bytes2(pairIndex),
            bytes32(order.minPrice),
            _encodeRecipient(order.recipient),
            _encodeHookData(order.hook, order.hookPayload),
            bytes16(order.amount),
            bytes16(order.maxGasAsset0),
            bytes16(order.gasUsedAsset0),
            _encodeSig(order.meta)
        );
    }

    function encode(TopOfBlockOrder[] memory orders, Pair[] memory pairs)
        internal
        pure
        returns (bytes memory b)
    {
        for (uint256 i = 0; i < orders.length; i++) {
            b = bytes.concat(b, orders[i].encode(pairs));
        }
        b = bytes.concat(bytes3(b.length.toUint24()), b);
    }

    function encode(TopOfBlockOrder memory order, Pair[] memory pairs)
        internal
        pure
        returns (bytes memory)
    {
        (uint16 pairIndex, bool zeroForOne) = pairs.getIndex(order.assetIn, order.assetOut);

        uint8 varMap = (order.useInternal ? 1 : 0) | (zeroForOne ? 2 : 0)
            | (order.recipient != address(0) ? 4 : 0) | (order.meta.isEcdsa ? 8 : 0);

        return bytes.concat(
            bytes1(varMap),
            bytes16(order.quantityIn),
            bytes16(order.quantityOut),
            bytes16(order.maxGasAsset0),
            bytes16(order.gasUsedAsset0),
            bytes2(pairIndex),
            _encodeRecipient(order.recipient),
            _encodeSig(order.meta)
        );
    }

    function toStr(PartialStandingOrder memory o) internal pure returns (string memory str) {
        str = string.concat(
            "PartialStandingOrder {",
            "\n  minAmountIn: ",
            o.minAmountIn.toStr(),
            ",\n  maxAmountIn: ",
            o.maxAmountIn.toStr(),
            ",\n  minPrice: ",
            o.minPrice.toStr(),
            ",\n  useInternal: ",
            o.useInternal.toStr(),
            ",\n  assetIn: ",
            o.assetIn.toStr(),
            ",\n  assetOut: ",
            o.assetOut.toStr(),
            ",\n  recipient: "
        );
        str = string.concat(
            str,
            o.recipient.toStr(),
            ",\n  hook: ",
            o.hook.toStr(),
            ",\n  hookPayload: ",
            o.hookPayload.toStr(),
            ",\n  nonce: ",
            o.nonce.toStr(),
            ",\n  deadline: ",
            o.deadline.toStr(),
            ",\n  amountFilled: ",
            o.amountFilled.toStr(),
            ",\n  meta: ",
            o.meta.toStr(),
            "\n}"
        );
    }

    function toStr(ExactStandingOrder memory o) internal pure returns (string memory str) {
        str = string.concat(
            "ExactStandingOrder {",
            "\n  exactIn: ",
            o.exactIn.toStr(),
            ",\n  amount: ",
            o.amount.toStr(),
            ",\n  minPrice: ",
            o.minPrice.toStr(),
            ",\n  useInternal: ",
            o.useInternal.toStr(),
            ",\n  assetIn: ",
            o.assetIn.toStr(),
            ",\n  assetOut: ",
            o.assetOut.toStr()
        );
        str = string.concat(
            str,
            ",\n  recipient: ",
            o.recipient.toStr(),
            ",\n  hook: ",
            o.hook.toStr(),
            ",\n  hookPayload: ",
            o.hookPayload.toStr(),
            ",\n  nonce: ",
            o.nonce.toStr(),
            ",\n  deadline: ",
            o.deadline.toStr(),
            ",\n  meta: ",
            o.meta.toStr(),
            "\n}"
        );
    }

    function toStr(PartialFlashOrder memory o) internal pure returns (string memory str) {
        str = string.concat(
            "PartialFlashOrder {",
            "\n  minAmountIn: ",
            o.minAmountIn.toStr(),
            ",\n  maxAmountIn: ",
            o.maxAmountIn.toStr(),
            ",\n  minPrice: ",
            o.minPrice.toStr(),
            ",\n  useInternal: ",
            o.useInternal.toStr(),
            ",\n  assetIn: ",
            o.assetIn.toStr(),
            ",\n  assetOut: ",
            o.assetOut.toStr()
        );
        str = string.concat(
            str,
            ",\n  recipient: ",
            o.recipient.toStr(),
            ",\n  hook: ",
            o.hook.toStr(),
            ",\n  hookPayload: ",
            o.hookPayload.toStr(),
            ",\n  validForBlock: ",
            o.validForBlock.toStr(),
            ",\n  amountFilled: ",
            o.amountFilled.toStr(),
            ",\n  meta: ",
            o.meta.toStr(),
            "\n}"
        );
    }

    function toStr(ExactFlashOrder memory o) internal pure returns (string memory str) {
        str = string.concat(
            "ExactFlashOrder {",
            "\n  exactIn: ",
            o.exactIn.toStr(),
            ",\n  amount: ",
            o.amount.toStr(),
            ",\n  minPrice: ",
            o.minPrice.toStr(),
            ",\n  useInternal: ",
            o.useInternal.toStr(),
            ",\n  assetIn: ",
            o.assetIn.toStr(),
            ",\n  assetOut: "
        );
        str = string.concat(
            str,
            o.assetOut.toStr(),
            ",\n  recipient: ",
            o.recipient.toStr(),
            ",\n  hook: ",
            o.hook.toStr(),
            ",\n  hookPayload: ",
            o.hookPayload.toStr(),
            ",\n  validForBlock: ",
            o.validForBlock.toStr(),
            ",\n  meta: ",
            o.meta.toStr(),
            "\n}"
        );
    }

    function toStr(OrderMeta memory meta) internal pure returns (string memory) {
        return string.concat(
            "OrderMeta { isEcdsa: ",
            meta.isEcdsa.toStr(),
            ", from: ",
            meta.from.toStr(),
            ", signature: ",
            meta.signature.toStr(),
            " }"
        );
    }

    function _encodeHookData(address hook, bytes memory hookPayload)
        internal
        pure
        returns (bytes memory)
    {
        if (hook == address(0)) {
            return new bytes(0);
        }
        return
            bytes.concat(bytes3((hookPayload.length + 20).toUint24()), bytes20(hook), hookPayload);
    }

    function _toHookData(address hook, bytes memory hookPayload)
        internal
        pure
        returns (bytes memory)
    {
        if (hook == address(0)) {
            return new bytes(0);
        }
        return bytes.concat(bytes20(hook), hookPayload);
    }

    function _encodeRecipient(address recipient) internal pure returns (bytes memory) {
        return recipient == address(0) ? new bytes(0) : bytes.concat(bytes20(recipient));
    }

    function _encodeSig(OrderMeta memory meta) internal pure returns (bytes memory) {
        if (meta.isEcdsa) {
            return meta.signature;
        } else {
            // ERC1271
            return bytes.concat(
                bytes20(meta.from), bytes3(meta.signature.length.toUint24()), meta.signature
            );
        }
    }
}
