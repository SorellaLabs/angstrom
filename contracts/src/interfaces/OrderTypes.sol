// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {AssetIndex} from "../libraries/PriceGraph.sol";

enum AssetForm {
    Liquid,
    UniV4Claim,
    AngstromClaim
}

enum OrderMode {
    ExactIn,
    ExactOut,
    Partial
}

using OrderLib for OrderMode global;

enum OrderType {
    Flash,
    Standing
}

struct GenericOrder {
    OrderType otype;
    OrderMode mode;
    uint256 amountSpecified;
    uint256 minPrice;
    AssetIndex assetInIndex;
    AssetForm assetInForm;
    AssetIndex assetOutIndex;
    AssetForm assetOutForm;
    uint64 nonce;
    uint256 deadline;
    address recipient;
    address hook;
    bytes hookPayload;
    uint256 amountFilled;
    address from;
    bytes signature;
}

using OrderLib for GenericOrder global;

struct TopOfBlockOrderEnvelope {
    uint256 amountIn;
    uint256 amountOut;
    AssetIndex assetInIndex;
    AssetForm assetInForm;
    AssetIndex assetOutIndex;
    AssetForm assetOutForm;
    address recipient;
    address hook;
    bytes hookPayload;
    address from;
    bytes signature;
}

using OrderLib for TopOfBlockOrderEnvelope global;

library OrderLib {
    error CannotFlashloanZero();

    /// forgefmt: disable-next-item
    bytes32 internal constant STANDING_ORDER_TYPEHASH = keccak256(
        "StandingOrder("
           "string mode,"
           "uint256 max_amount_in_or_out,"
           "uint256 min_price,"
           "address asset_in,"
           "uint8 asset_in_form,"
           "address asset_out,"
           "uint8 asset_out_form,"
           "address recipient,"
           "bytes hook_data,"
           "uint64 nonce,"
           "uint256 deadline"
        ")"
    );

    /// forgefmt: disable-next-item
    bytes32 internal constant FLASH_ORDER_TYPEHASH = keccak256(
        "FlashOrder("
           "string mode,"
           "uint256 max_amount_in_or_out,"
           "uint256 min_price,"
           "address asset_in,"
           "uint8 asset_in_form,"
           "address asset_out,"
           "uint8 asset_out_form,"
           "address recipient,"
           "bytes hook_data,"
           "uint64 valid_for_block"
        ")"
    );

    /// forgefmt: disable-next-item
    bytes32 internal constant TOP_OF_BLOCK_ORDER_TYPEHASH = keccak256(
        "TopOfBlockOrder("
           "uint256 amountIn,"
           "uint256 amountOut,"
           "address assetIn,"
           "uint8 assetInForm,"
           "address assetOut,"
           "uint8 assetOutForm,"
           "address recipient,"
           "bytes hookData,"
           "uint256 validForBlock"
        ")"
    );

    function hash(GenericOrder memory order, address assetIn, address assetOut) internal view returns (bytes32) {
        if (order.otype == OrderType.Standing) {
            return keccak256(
                abi.encode(
                    STANDING_ORDER_TYPEHASH,
                    order.mode.hash(),
                    order.amountSpecified,
                    order.minPrice,
                    assetIn,
                    order.assetInForm,
                    assetOut,
                    order.assetOutForm,
                    order.recipient,
                    _hashHookData(order.hook, order.hookPayload),
                    order.nonce,
                    order.deadline
                )
            );
        }
        if (order.otype == OrderType.Flash) {
            return keccak256(
                abi.encode(
                    FLASH_ORDER_TYPEHASH,
                    order.mode.hash(),
                    order.amountSpecified,
                    order.minPrice,
                    assetIn,
                    order.assetInForm,
                    assetOut,
                    order.assetOutForm,
                    order.recipient,
                    _hashHookData(order.hook, order.hookPayload),
                    block.number
                )
            );
        }
        assert(false);
        return bytes32(0);
    }

    function hash(TopOfBlockOrderEnvelope memory order, address assetIn, address assetOut)
        internal
        view
        returns (bytes32)
    {
        return keccak256(
            abi.encode(
                TOP_OF_BLOCK_ORDER_TYPEHASH,
                order.amountIn,
                order.amountOut,
                assetIn,
                order.assetInForm,
                assetOut,
                order.assetOutForm,
                order.recipient,
                _hashHookData(order.hook, order.hookPayload),
                block.number
            )
        );
    }

    function hash(OrderMode mode) internal pure returns (bytes32) {
        if (mode == OrderMode.ExactIn) {
            return keccak256("ExactIn");
        }
        if (mode == OrderMode.ExactOut) {
            return keccak256("ExactOut");
        }
        if (mode == OrderMode.Partial) {
            return keccak256("ExactOut");
        }
        assert(false);
        return bytes32(0);
    }

    function _hashHookData(address hook, bytes memory hookPayload) internal pure returns (bytes32) {
        if (hook == address(0)) {
            return keccak256("");
        }
        return keccak256(abi.encodePacked(hook, hookPayload));
    }
}
