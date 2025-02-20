// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import {UserOrderBufferLib} from "src/types/UserOrderBuffer.sol";
import {ToBOrderBufferLib} from "src/types/ToBOrderBuffer.sol";
import {TWAPOrderBufferLib} from "src/types/TWAPOrderBuffer.sol";

struct PartialStandingOrder {
    uint32 ref_id;
    uint128 min_amount_in;
    uint128 max_amount_in;
    uint128 max_extra_fee_asset0;
    uint256 min_price;
    bool use_internal;
    address asset_in;
    address asset_out;
    address recipient;
    bytes hook_data;
    uint64 nonce;
    uint40 deadline;
}

struct ExactStandingOrder {
    uint32 ref_id;
    bool exact_in;
    uint128 amount;
    uint128 max_extra_fee_asset0;
    uint256 min_price;
    bool use_internal;
    address asset_in;
    address asset_out;
    address recipient;
    bytes hook_data;
    uint64 nonce;
    uint40 deadline;
}

struct PartialFlashOrder {
    uint32 ref_id;
    uint128 min_amount_in;
    uint128 max_amount_in;
    uint128 max_extra_fee_asset0;
    uint256 min_price;
    bool use_internal;
    address asset_in;
    address asset_out;
    address recipient;
    bytes hook_data;
    uint64 valid_for_block;
}

struct ExactFlashOrder {
    uint32 ref_id;
    bool exact_in;
    uint128 amount;
    uint128 max_extra_fee_asset0;
    uint256 min_price;
    bool use_internal;
    address asset_in;
    address asset_out;
    address recipient;
    bytes hook_data;
    uint64 valid_for_block;
}

struct TopOfBlockOrder {
    uint128 quantity_in;
    uint128 quantity_out;
    uint128 max_gas_asset0;
    bool use_internal;
    address asset_in;
    address asset_out;
    address recipient;
    uint64 valid_for_block;
}

struct TimeWeightedAveragePriceOrder {
    uint32 ref_id;
    bool exact_in;
    uint128 amount;
    uint128 max_extra_fee_asset0;
    uint256 min_price;
    bool use_internal;
    address asset_in;
    address asset_out;
    address recipient;
    bytes hook_data;
    uint64 nonce;
    uint40 start_time;
    uint32 total_parts;
    uint32 time_interval;
    uint32 window;
}

using SignedTypesLib for ExactStandingOrder global;
using SignedTypesLib for PartialStandingOrder global;
using SignedTypesLib for ExactFlashOrder global;
using SignedTypesLib for PartialFlashOrder global;
using SignedTypesLib for TopOfBlockOrder global;
using SignedTypesLib for TimeWeightedAveragePriceOrder global;

/// @author philogy <https://github.com/philogy>
library SignedTypesLib {
    function hash(PartialStandingOrder memory self) internal pure returns (bytes32) {
        return keccak256(
            abi.encode(
                UserOrderBufferLib.PARTIAL_STANDING_ORDER_TYPEHASH,
                self.ref_id,
                self.min_amount_in,
                self.max_amount_in,
                self.max_extra_fee_asset0,
                self.min_price,
                self.use_internal,
                self.asset_in,
                self.asset_out,
                self.recipient,
                keccak256(self.hook_data),
                self.nonce,
                self.deadline
            )
        );
    }

    function hash(ExactStandingOrder memory self) internal pure returns (bytes32) {
        return keccak256(
            abi.encode(
                UserOrderBufferLib.EXACT_STANDING_ORDER_TYPEHASH,
                self.ref_id,
                self.exact_in,
                self.amount,
                self.max_extra_fee_asset0,
                self.min_price,
                self.use_internal,
                self.asset_in,
                self.asset_out,
                self.recipient,
                keccak256(self.hook_data),
                self.nonce,
                self.deadline
            )
        );
    }

    function hash(PartialFlashOrder memory self) internal pure returns (bytes32) {
        return keccak256(
            abi.encode(
                UserOrderBufferLib.PARTIAL_FLASH_ORDER_TYPEHASH,
                self.ref_id,
                self.min_amount_in,
                self.max_amount_in,
                self.max_extra_fee_asset0,
                self.min_price,
                self.use_internal,
                self.asset_in,
                self.asset_out,
                self.recipient,
                keccak256(self.hook_data),
                self.valid_for_block
            )
        );
    }

    function hash(ExactFlashOrder memory self) internal pure returns (bytes32) {
        return keccak256(
            abi.encode(
                UserOrderBufferLib.EXACT_FLASH_ORDER_TYPEHASH,
                self.ref_id,
                self.exact_in,
                self.amount,
                self.max_extra_fee_asset0,
                self.min_price,
                self.use_internal,
                self.asset_in,
                self.asset_out,
                self.recipient,
                keccak256(self.hook_data),
                self.valid_for_block
            )
        );
    }

    function hash(TopOfBlockOrder memory self) internal pure returns (bytes32) {
        return keccak256(
            abi.encode(
                ToBOrderBufferLib.TOP_OF_BLOCK_ORDER_TYPEHASH,
                self.quantity_in,
                self.quantity_out,
                self.max_gas_asset0,
                self.use_internal,
                self.asset_in,
                self.asset_out,
                self.recipient,
                self.valid_for_block
            )
        );
    }

    struct TimeWeightedAveragePriceOrderMem {
        bytes32 type_hash;
        uint32 ref_id;
        bool exact_in;
        uint128 amount;
        uint128 max_extra_fee_asset0;
        uint256 min_price;
        bool use_internal;
        address asset_in;
        address asset_out;
        address recipient;
        bytes32 hook_data_hash;
        uint64 nonce;
        uint40 start_time;
        uint32 total_parts;
        uint32 time_interval;
        uint32 window;
    }

    function hash(TimeWeightedAveragePriceOrder memory self) internal pure returns (bytes32) {
        TimeWeightedAveragePriceOrderMem memory orderToMem = TimeWeightedAveragePriceOrderMem({
            type_hash: TWAPOrderBufferLib.TWAP_ORDER_TYPEHASH,
            ref_id: self.ref_id,
            exact_in: self.exact_in,
            amount: self.amount,
            max_extra_fee_asset0: self.max_extra_fee_asset0,
            min_price: self.min_price,
            use_internal: self.use_internal,
            asset_in: self.asset_in,
            asset_out: self.asset_out,
            recipient: self.recipient,
            hook_data_hash: keccak256(self.hook_data),
            nonce: self.nonce,
            start_time: self.start_time,
            total_parts: self.total_parts,
            time_interval: self.time_interval,
            window: self.window
        });
        return keccak256(abi.encode(orderToMem));
    }
}
