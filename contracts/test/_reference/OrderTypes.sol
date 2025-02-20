// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {UserOrderVariantMap} from "src/types/UserOrderVariantMap.sol";
import {OrderVariant as RefOrderVariant} from "./OrderVariant.sol";
import {UserOrderBufferLib} from "src/types/UserOrderBuffer.sol";
import {ToBOrderBufferLib} from "src/types/ToBOrderBuffer.sol";
import {SafeCastLib} from "solady/src/utils/SafeCastLib.sol";
import {Pair, PairLib} from "./Pair.sol";
import {
    PartialStandingOrder as SignedPartialStandingOrder,
    ExactStandingOrder as SignedExactStandingOrder,
    PartialFlashOrder as SignedPartialFlashOrder,
    ExactFlashOrder as SignedExactFlashOrder,
    TopOfBlockOrder as SignedTopOfBlockOrder,
    TimeWeightedAveragePriceOrder as SignedTimeWeightedAveragePriceOrder
} from "./SignedTypes.sol";

import {FormatLib} from "super-sol/libraries/FormatLib.sol";

struct OrderMeta {
    bool isEcdsa;
    address from;
    bytes signature;
}

struct PartialStandingOrder {
    uint32 refId;
    uint128 minAmountIn;
    uint128 maxAmountIn;
    uint128 maxExtraFeeAsset0;
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
    uint128 extraFeeAsset0;
}

struct ExactStandingOrder {
    uint32 refId;
    bool exactIn;
    uint128 amount;
    uint128 maxExtraFeeAsset0;
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
    uint128 extraFeeAsset0;
}

struct PartialFlashOrder {
    uint32 refId;
    uint128 minAmountIn;
    uint128 maxAmountIn;
    uint128 maxExtraFeeAsset0;
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
    uint128 extraFeeAsset0;
}

struct ExactFlashOrder {
    uint32 refId;
    bool exactIn;
    uint128 amount;
    uint128 maxExtraFeeAsset0;
    uint256 minPrice;
    bool useInternal;
    address assetIn;
    address assetOut;
    address recipient;
    address hook;
    bytes hookPayload;
    uint64 validForBlock;
    OrderMeta meta;
    uint128 extraFeeAsset0;
}

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

struct TimeWeightedAveragePriceOrder {
    uint32 refId;
    bool exactIn;
    uint128 amount;
    uint128 maxExtraFeeAsset0;
    uint256 minPrice;
    bool useInternal;
    address assetIn;
    address assetOut;
    address recipient;
    address hook;
    bytes hookPayload;
    uint64 nonce;
    uint40 startTime;
    uint32 totalParts;
    uint32 timeInterval;
    uint32 window;
    OrderMeta meta;
    uint128 extraFeeAsset0;
}

using OrdersLib for OrderMeta global;
using OrdersLib for PartialStandingOrder global;
using OrdersLib for ExactStandingOrder global;
using OrdersLib for PartialFlashOrder global;
using OrdersLib for ExactFlashOrder global;
using OrdersLib for TopOfBlockOrder global;
using OrdersLib for TimeWeightedAveragePriceOrder global;

library OrdersLib {
    using PairLib for *;
    using FormatLib for *;
    using SafeCastLib for *;

    function hash(PartialStandingOrder memory order) internal pure returns (bytes32) {
        return SignedPartialStandingOrder(
            order.refId,
            order.minAmountIn,
            order.maxAmountIn,
            order.maxExtraFeeAsset0,
            order.minPrice,
            order.useInternal,
            order.assetIn,
            order.assetOut,
            order.recipient,
            _toHookData(order.hook, order.hookPayload),
            order.nonce,
            order.deadline
        ).hash();
    }

    function hash(ExactStandingOrder memory order) internal pure returns (bytes32) {
        return SignedExactStandingOrder(
            order.refId,
            order.exactIn,
            order.amount,
            order.maxExtraFeeAsset0,
            order.minPrice,
            order.useInternal,
            order.assetIn,
            order.assetOut,
            order.recipient,
            _toHookData(order.hook, order.hookPayload),
            order.nonce,
            order.deadline
        ).hash();
    }

    function hash(PartialFlashOrder memory order) internal pure returns (bytes32) {
        return SignedPartialFlashOrder(
            order.refId,
            order.minAmountIn,
            order.maxAmountIn,
            order.maxExtraFeeAsset0,
            order.minPrice,
            order.useInternal,
            order.assetIn,
            order.assetOut,
            order.recipient,
            _toHookData(order.hook, order.hookPayload),
            order.validForBlock
        ).hash();
    }

    function hash(ExactFlashOrder memory order) internal pure returns (bytes32) {
        return SignedExactFlashOrder(
            order.refId,
            order.exactIn,
            order.amount,
            order.maxExtraFeeAsset0,
            order.minPrice,
            order.useInternal,
            order.assetIn,
            order.assetOut,
            order.recipient,
            _toHookData(order.hook, order.hookPayload),
            order.validForBlock
        ).hash();
    }

    function hash(TopOfBlockOrder memory order) internal pure returns (bytes32) {
        return SignedTopOfBlockOrder(
            order.quantityIn,
            order.quantityOut,
            order.maxGasAsset0,
            order.useInternal,
            order.assetIn,
            order.assetOut,
            order.recipient,
            order.validForBlock
        ).hash();
    }

    function hash(TimeWeightedAveragePriceOrder memory order) internal pure returns (bytes32) {
        return SignedTimeWeightedAveragePriceOrder(
            order.refId,
            order.exactIn,
            order.amount,
            order.maxExtraFeeAsset0,
            order.minPrice,
            order.useInternal,
            order.assetIn,
            order.assetOut,
            order.recipient,
            _toHookData(order.hook, order.hookPayload),
            order.nonce,
            order.startTime,
            order.totalParts,
            order.timeInterval,
            order.window
        ).hash();
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
            bytes16(order.maxExtraFeeAsset0),
            bytes16(order.extraFeeAsset0),
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
            bytes16(order.maxExtraFeeAsset0),
            bytes16(order.extraFeeAsset0),
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
            bytes16(order.maxExtraFeeAsset0),
            bytes16(order.extraFeeAsset0),
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
            bytes16(order.maxExtraFeeAsset0),
            bytes16(order.extraFeeAsset0),
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

    function encode(TimeWeightedAveragePriceOrder[] memory orders, Pair[] memory pairs)
        internal
        pure
        returns (bytes memory b)
    {
        for (uint256 i = 0; i < orders.length; i++) {
            b = bytes.concat(b, orders[i].encode(pairs));
        }
        b = bytes.concat(bytes3(b.length.toUint24()), b);
    }

    function toVariantMap(TimeWeightedAveragePriceOrder memory order, bool zeroForOne)
        internal
        pure
        returns(uint8 varMap)
    {
        varMap = (order.useInternal ? 1 : 0) | (order.recipient != address(0) ? 2 : 0)
            | (order.hook != address(0) ? 4 : 0) | (zeroForOne ? 8 : 0)
            | (order.exactIn ? 16 : 0) | (order.meta.isEcdsa ? 32 : 0);
    }

    function encode(TimeWeightedAveragePriceOrder memory order, Pair[] memory pairs)
        internal
        pure
        returns (bytes memory)
    {
        (uint16 pairIndex, bool zeroForOne) = pairs.getIndex(order.assetIn, order.assetOut);

        return bytes.concat(
            bytes.concat(
                bytes1(order.toVariantMap(zeroForOne)),
                bytes4(order.refId),
                bytes2(pairIndex),
                bytes32(order.minPrice),
                _encodeRecipient(order.recipient),
                _encodeHookData(order.hook, order.hookPayload),
                bytes8(order.nonce)
            ),
            bytes5(order.startTime),
            bytes4(order.totalParts),
            bytes4(order.timeInterval),
            bytes4(order.window),
            bytes16(order.amount),
            bytes16(order.maxExtraFeeAsset0),
            bytes16(order.extraFeeAsset0),
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

    function toStr(TimeWeightedAveragePriceOrder memory o) internal pure returns (string memory str) {
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
            ",\n  startTime: ",
            o.startTime.toStr(),
            ",\n  totalParts: ",
            o.totalParts.toStr(),
            ",\n  timeInterval: ",
            o.timeInterval.toStr(),
            ",\n  window: ",
            o.window.toStr(),
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