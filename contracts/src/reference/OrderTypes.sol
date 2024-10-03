// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {UserOrderVariantMap} from "../types/UserOrderVariantMap.sol";
import {OrderVariant as RefOrderVariant} from "../reference/OrderVariant.sol";
import {UserOrderBufferLib} from "../types/UserOrderBuffer.sol";
import {SafeCastLib} from "solady/src/utils/SafeCastLib.sol";
import {Pair, PairLib} from "./Pair.sol";

import {FormatLib} from "super-sol/libraries/FormatLib.sol";
import {console} from "forge-std/console.sol";

struct OrderMeta {
    bool isEcdsa;
    address from;
    bytes signature;
}

struct PartialStandingOrder {
    uint128 minAmountIn;
    uint128 maxAmountIn;
    uint128 maxGasAsset0;
    uint128 gasUsedAsset0;
    uint256 minPrice;
    bool useInternal;
    address assetIn;
    address assetOut;
    address recipient;
    address hook;
    bytes hookPayload;
    uint64 nonce;
    uint40 deadline;
    uint128 amountFilled;
    OrderMeta meta;
}

struct ExactStandingOrder {
    bool exactIn;
    uint128 amount;
    uint128 maxGasAsset0;
    uint128 gasUsedAsset0;
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
}

struct PartialFlashOrder {
    uint128 minAmountIn;
    uint128 maxAmountIn;
    uint128 maxGasAsset0;
    uint128 gasUsedAsset0;
    uint256 minPrice;
    bool useInternal;
    address assetIn;
    address assetOut;
    address recipient;
    address hook;
    bytes hookPayload;
    uint64 validForBlock;
    uint128 amountFilled;
    OrderMeta meta;
}

struct ExactFlashOrder {
    bool exactIn;
    uint128 amount;
    uint128 maxGasAsset0;
    uint128 gasUsedAsset0;
    uint256 minPrice;
    bool useInternal;
    address assetIn;
    address assetOut;
    address recipient;
    address hook;
    bytes hookPayload;
    uint64 validForBlock;
    OrderMeta meta;
}

struct TopOfBlockOrder {
    uint128 quantityIn;
    uint128 quantityOut;
    uint128 maxGasAsset0;
    uint128 gasUsedAsset0;
    bool useInternal;
    address assetIn;
    address assetOut;
    address recipient;
    address hook;
    bytes hookPayload;
    uint64 validForBlock;
    OrderMeta meta;
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

    /// forgefmt: disable-next-item
    bytes32 internal constant PARTIAL_STANDING_ORDER_TYPEHASH = keccak256(
        "PartialStandingOrder("
           "uint128 min_amount_in,"
           "uint128 max_amount_in,"
           "uint128 max_gas_asset0,"
           "uint256 min_price,"
           "bool use_internal,"
           "address asset_in,"
           "address asset_out,"
           "address recipient,"
           "bytes hook_data,"
           "uint64 nonce,"
           "uint40 deadline"
        ")"
    );

    /// forgefmt: disable-next-item
    bytes32 internal constant EXACT_STANDING_ORDER_TYPEHASH = keccak256(
        "ExactStandingOrder("
           "bool exact_in,"
           "uint128 amount,"
           "uint128 max_gas_asset0,"
           "uint256 min_price,"
           "bool use_internal,"
           "address asset_in,"
           "address asset_out,"
           "address recipient,"
           "bytes hook_data,"
           "uint64 nonce,"
           "uint40 deadline"
        ")"
    );

    /// forgefmt: disable-next-item
    bytes32 internal constant PARTIAL_FLASH_ORDER_TYPEHASH = keccak256(
        "PartialFlashOrder("
           "uint128 min_amount_in,"
           "uint128 max_amount_in,"
           "uint128 max_gas_asset0,"
           "uint256 min_price,"
           "bool use_internal,"
           "address asset_in,"
           "address asset_out,"
           "address recipient,"
           "bytes hook_data,"
           "uint64 valid_for_block"
        ")"
    );

    /// forgefmt: disable-next-item
    bytes32 internal constant EXACT_FLASH_ORDER_TYPEHASH = keccak256(
        "ExactFlashOrder("
           "bool exact_in,"
           "uint128 amount,"
           "uint128 max_gas_asset0,"
           "uint256 min_price,"
           "bool use_internal,"
           "address asset_in,"
           "address asset_out,"
           "address recipient,"
           "bytes hook_data,"
           "uint64 valid_for_block"
        ")"
    );

    /// forgefmt: disable-next-item
    bytes32 internal constant TOP_OF_BLOCK_ORDER_TYPEHASH = keccak256(
        "TopOfBlockOrder("
           "uint128 quantity_in,"
           "uint128 quantity_out,"
           "uint128 max_gas_asset0,"
           "bool use_internal,"
           "address asset_in,"
           "address asset_out,"
           "address recipient,"
           "bytes hook_data,"
           "uint256 valid_for_block"
        ")"
    );

    function hash(PartialStandingOrder memory order) internal pure returns (bytes32) {
        return keccak256(
            abi.encode(
                PARTIAL_STANDING_ORDER_TYPEHASH,
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
                EXACT_STANDING_ORDER_TYPEHASH,
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
                PARTIAL_FLASH_ORDER_TYPEHASH,
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
                EXACT_FLASH_ORDER_TYPEHASH,
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
                TOP_OF_BLOCK_ORDER_TYPEHASH,
                order.quantityIn,
                order.quantityOut,
                order.maxGasAsset0,
                order.useInternal,
                order.assetIn,
                order.assetOut,
                order.recipient,
                keccak256(_toHookData(order.hook, order.hookPayload)),
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
            | (order.recipient != address(0) ? 4 : 0) | (order.hook != address(0) ? 8 : 0)
            | (order.meta.isEcdsa ? 16 : 0);

        return bytes.concat(
            bytes1(varMap),
            bytes16(order.quantityIn),
            bytes16(order.quantityOut),
            bytes16(order.maxGasAsset0),
            bytes16(order.gasUsedAsset0),
            bytes2(pairIndex),
            _encodeRecipient(order.recipient),
            _encodeHookData(order.hook, order.hookPayload),
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
