// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import {CalldataReader} from "./CalldataReader.sol";
import {UserOrderVariantMap} from "./UserOrderVariantMap.sol";
import {TypedDataHasher} from "./TypedDataHasher.sol";
import {PriceAB as PriceOutVsIn, AmountA as AmountOut, AmountB as AmountIn} from "./Price.sol";

struct UserOrderBuffer {
    bytes32 typeHash;
    uint32 refId;
    uint256 exactIn_or_minQuantityIn;
    uint256 quantity_or_maxQuantityIn;
    uint256 maxGasAsset0;
    uint256 minPrice;
    bool useInternal;
    address assetIn;
    address assetOut;
    address recipient;
    bytes32 hookDataHash;
    uint64 nonce_or_validForBlock;
    uint40 deadline_or_empty;
}

using UserOrderBufferLib for UserOrderBuffer global;

/// @author philogy <https://github.com/philogy>
library UserOrderBufferLib {
    error FillingTooLittle();
    error FillingTooMuch();
    error GasAboveMax();

    // TODO: Make test that ensures that buffer space is always enough.
    uint256 internal constant STANDING_ORDER_BYTES = 416;
    uint256 internal constant FLASH_ORDER_BYTES = 384;

    uint256 internal constant VARIANT_MAP_BYTES = 1;
    uint256 internal constant REF_ID_MEM_OFFSET = 0x3c;
    uint256 internal constant REF_ID_BYTES = 4;

    uint256 internal constant NONCE_MEM_OFFSET = 0x160;
    uint256 internal constant NONCE_BYTES = 8;
    uint256 internal constant DEADLINE_MEM_OFFSET = 0x180;
    uint256 internal constant DEADLINE_BYTES = 5;

    /// forgefmt: disable-next-item
    bytes32 internal constant PARTIAL_STANDING_ORDER_TYPEHASH = keccak256(
        "PartialStandingOrder("
           "uint32 ref_id,"
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
           "uint32 ref_id,"
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
           "uint32 ref_id,"
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
           "uint32 ref_id,"
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

    function init(UserOrderBuffer memory self, CalldataReader reader)
        internal
        pure
        returns (CalldataReader, UserOrderVariantMap variantMap)
    {
        assembly ("memory-safe") {
            variantMap := byte(0, calldataload(reader))
            reader := add(reader, VARIANT_MAP_BYTES)
            // Copy `refId` from calldata directly to memory.
            calldatacopy(add(self, REF_ID_MEM_OFFSET), reader, REF_ID_BYTES)
            // Advance reader.
            reader := add(reader, REF_ID_BYTES)
        }
        // forgefmt: disable-next-item
        if (variantMap.quantitiesPartial()) {
            self.typeHash = variantMap.isStanding()
                ? PARTIAL_STANDING_ORDER_TYPEHASH
                : PARTIAL_FLASH_ORDER_TYPEHASH;
        } else {
            self.typeHash = variantMap.isStanding()
                ? EXACT_STANDING_ORDER_TYPEHASH
                : EXACT_FLASH_ORDER_TYPEHASH;
        }

        self.useInternal = variantMap.useInternal();

        return (reader, variantMap);
    }

    function structHash(UserOrderBuffer memory self, UserOrderVariantMap variant)
        internal
        pure
        returns (bytes32 hashed)
    {
        uint256 structLength = variant.isStanding() ? STANDING_ORDER_BYTES : FLASH_ORDER_BYTES;
        assembly ("memory-safe") {
            hashed := keccak256(self, structLength)
        }
    }

    function loadAndComputeQuantity(
        UserOrderBuffer memory self,
        CalldataReader reader,
        UserOrderVariantMap variant,
        PriceOutVsIn price
    ) internal pure returns (CalldataReader, AmountIn quantityIn, AmountOut quantityOut) {
        uint256 quantity;
        if (variant.quantitiesPartial()) {
            uint256 minQuantityIn;
            uint256 maxQuantityIn;
            (reader, minQuantityIn) = reader.readU128();
            (reader, maxQuantityIn) = reader.readU128();
            (reader, quantity) = reader.readU128();
            self.exactIn_or_minQuantityIn = minQuantityIn;
            self.quantity_or_maxQuantityIn = maxQuantityIn;

            if (quantity < minQuantityIn) revert FillingTooLittle();
            if (quantity > maxQuantityIn) revert FillingTooMuch();
        } else {
            // Partial order.
            (reader, quantity) = reader.readU128();
            self.exactIn_or_minQuantityIn = variant.exactIn() ? 1 : 0;
            self.quantity_or_maxQuantityIn = quantity;
        }

        uint128 gasUsedAsset0;
        {
            uint128 maxGasAsset0;
            (reader, maxGasAsset0) = reader.readU128();
            (reader, gasUsedAsset0) = reader.readU128();
            if (gasUsedAsset0 > maxGasAsset0) revert GasAboveMax();
            self.maxGasAsset0 = maxGasAsset0;
        }

        if (variant.zeroForOne()) {
            if (variant.specifyingInput()) {
                quantityIn = AmountIn.wrap(quantity - gasUsedAsset0);
                quantityOut = price.convert(quantityIn);
            } else {
                quantityOut = AmountOut.wrap(quantity);
                quantityIn = price.convert(quantityOut) - AmountIn.wrap(gasUsedAsset0);
            }
        } else {
            if (variant.specifyingInput()) {
                quantityIn = AmountIn.wrap(quantity);
                quantityOut = price.convert(quantityIn) - AmountOut.wrap(gasUsedAsset0);
            } else {
                quantityOut = AmountOut.wrap(quantity - gasUsedAsset0);
                quantityIn = price.convert(quantityOut);
            }
        }

        return (reader, quantityIn, quantityOut);
    }

    function readOrderValidation(
        UserOrderBuffer memory self,
        CalldataReader reader,
        UserOrderVariantMap variant
    ) internal view returns (CalldataReader) {
        if (variant.isStanding()) {
            // Copy slices directly from calldata into memory.
            assembly ("memory-safe") {
                calldatacopy(
                    add(self, add(NONCE_MEM_OFFSET, sub(0x20, NONCE_BYTES))), reader, NONCE_BYTES
                )
                reader := add(reader, NONCE_BYTES)
                calldatacopy(
                    add(self, add(DEADLINE_MEM_OFFSET, sub(0x20, DEADLINE_BYTES))),
                    reader,
                    DEADLINE_BYTES
                )
                reader := add(reader, DEADLINE_BYTES)
            }
        } else {
            // Nothing loaded from calldata, reader stays unmodified.
            self.nonce_or_validForBlock = uint64(block.number);
        }
        return reader;
    }
}
