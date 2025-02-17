// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import {CalldataReader} from "./CalldataReader.sol";
import {TWAPOrderVariantMap} from "./TWAPOrderVariantMap.sol";
import {PriceAB as PriceOutVsIn, AmountA as AmountOut, AmountB as AmountIn} from "./Price.sol";

struct TWAPOrderBuffer {
    bytes32 typeHash;
    uint32 refId;
    bool exactIn;
    uint256 quantity;
    uint256 maxExtraFeeAsset0;
    uint256 minPrice;
    bool useInternal;
    address assetIn;
    address assetOut;
    address recipient;
    bytes32 hookDataHash;
    uint64 nonce;
    uint40 startTime;
    uint32 totalParts;
    uint32 timeInterval;
}

using TWAPOrderBufferLib for TWAPOrderBuffer global;

/// @author philogy <https://github.com/philogy>
library TWAPOrderBufferLib {
    error GasAboveMax();

    uint256 internal constant BUFFER_BYTES = 480;

    uint256 internal constant VARIANT_MAP_BYTES = 1;
    /// @dev Destination offset for direct calldatacopy of 4-byte ref ID (therefore not word aligned).
    uint256 internal constant REF_ID_MEM_OFFSET = 0x20;
    uint256 internal constant REF_ID_BYTES = 4;
    uint256 internal constant NONCE_MEM_OFFSET = 0x160;
    uint256 internal constant NONCE_BYTES = 8;
    uint256 internal constant START_TIME_MEM_OFFSET = 0x180;
    uint256 internal constant START_TIME_BYTES = 5;
    uint256 internal constant PARTS_MEM_OFFSET = 0x1a0;
    uint256 internal constant PARTS_BYTES = 4;
    uint256 internal constant TIME_INTERVALS_MEM_OFFSET = 0x1c0;
    uint256 internal constant TIME_INTERVALS_BYTES = 4;

    /// forgefmt: disable-next-item
    bytes32 internal constant TWAP_ORDER_TYPEHASH = keccak256(
        "TimeWeightedAveragePriceOrder("
           "uint32 ref_id,"
           "bool exact_in,"
           "uint128 amount,"
           "uint128 max_extra_fee_asset0,"
           "uint256 min_price,"
           "bool use_internal,"
           "address asset_in,"
           "address asset_out,"
           "address recipient,"
           "bytes hook_data,"
           "uint64 nonce,"
           "uint40 start_time,"
           "uint32 total_parts,"
           "uint32 time_interval"
        ")"
    );

    function init(TWAPOrderBuffer memory self, CalldataReader reader)
        internal
        pure
        returns (CalldataReader, TWAPOrderVariantMap variantMap)
    {
        assembly ("memory-safe") {
            variantMap := byte(0, calldataload(reader))
            reader := add(reader, VARIANT_MAP_BYTES)
            // Copy `refId` from calldata directly to memory.
            calldatacopy(
                add(self, add(REF_ID_MEM_OFFSET, sub(0x20, REF_ID_BYTES))), reader, REF_ID_BYTES
            )
            // Advance reader.
            reader := add(reader, REF_ID_BYTES)
        }
        
        self.typeHash = TWAP_ORDER_TYPEHASH;
        
        self.useInternal = variantMap.useInternal();

        return (reader, variantMap);
   
    }

    function hash(TWAPOrderBuffer memory self) internal pure returns (bytes32 orderHash) {
        assembly ("memory-safe") {
            orderHash := keccak256(self, BUFFER_BYTES)
        }
    }

    function loadAndComputeQuantity(
        TWAPOrderBuffer memory self,
        CalldataReader reader,
        TWAPOrderVariantMap variant,
        PriceOutVsIn price
    ) internal pure returns (CalldataReader, AmountIn quantityIn, AmountOut quantityOut) {
        uint256 quantity;
        (reader, quantity) = reader.readU128();
        // how is this actually used.
        // self.exactIn = variant.exactIn();
        self.quantity = quantity;

        uint128 extraFeeAsset0;
        uint128 maxExtraFeeAsset0;
        (reader, maxExtraFeeAsset0) = reader.readU128();
        (reader, extraFeeAsset0) = reader.readU128();
        if (extraFeeAsset0 > maxExtraFeeAsset0) revert GasAboveMax();
        self.maxExtraFeeAsset0 = maxExtraFeeAsset0;

        quantityIn = AmountIn.wrap(quantity);
        if (variant.zeroForOne()) {
            AmountIn fee = AmountIn.wrap(extraFeeAsset0);
            quantityOut = price.convertDown(quantityIn - fee);
        } else {
            AmountOut fee = AmountOut.wrap(extraFeeAsset0);
            quantityOut = price.convertDown(quantityIn) - fee;
        }

        return (reader, quantityIn, quantityOut);
    }

    function readOrderValidation(
        TWAPOrderBuffer memory self,
        CalldataReader reader
    ) internal pure returns (CalldataReader) {
        // Copy slices directly from calldata into memory.
        assembly ("memory-safe") {
            calldatacopy(
                add(self, add(NONCE_MEM_OFFSET, sub(0x20, NONCE_BYTES))), reader, NONCE_BYTES
            )
            reader := add(reader, NONCE_BYTES)
            calldatacopy(
                add(self, add(START_TIME_MEM_OFFSET, sub(0x20, START_TIME_BYTES))),
                reader,
                START_TIME_BYTES
            )
            reader := add(reader, START_TIME_BYTES)
            calldatacopy(
                add(self, add(PARTS_MEM_OFFSET, sub(0x20, PARTS_BYTES))),
                reader,
                PARTS_BYTES
            )
            reader := add(reader, PARTS_BYTES)
            calldatacopy(
                add(self, add(TIME_INTERVALS_MEM_OFFSET, sub(0x20, TIME_INTERVALS_BYTES))),
                reader,
                TIME_INTERVALS_BYTES
            )
            reader := add(reader, TIME_INTERVALS_BYTES)
        }
        return reader;
    }
}