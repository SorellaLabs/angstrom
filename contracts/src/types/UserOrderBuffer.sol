// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import {OrdersLib} from "../reference/OrderTypes.sol";
import {CalldataReader} from "./CalldataReader.sol";
import {OrderVariantMap} from "./OrderVariantMap.sol";
import {TypedDataHasher} from "./TypedDataHasher.sol";
import {PriceAB as PriceOutVsIn, AmountA as AmountOut, AmountB as AmountIn} from "./Price.sol";

import {DEBUG_LOGS} from "../modules/DevFlags.sol";

import {safeconsole as console} from "forge-std/safeconsole.sol";
import {consoleext} from "super-sol/libraries/consoleext.sol";

struct UserOrderBuffer {
    bytes32 typeHash;
    uint256 exactIn_or_minQuantityIn;
    uint256 quantity_or_maxQuantityIn;
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

    // TODO: Make test that ensures that buffer space is always enough.
    uint256 internal constant STANDING_ORDER_BYTES = 352;
    uint256 internal constant FLASH_ORDER_BYTES = 320;

    function setTypeHash(UserOrderBuffer memory self, OrderVariantMap variant) internal pure {
        if (variant.quantitiesPartial()) {
            if (variant.isStanding()) {
                self.typeHash = OrdersLib.PARTIAL_STANDING_ORDER_TYPEHASH;
            } else {
                // is flash order.
                self.typeHash = OrdersLib.PARTIAL_FLASH_ORDER_TYPEHASH;
            }
        } else {
            // exact order.
            if (variant.isStanding()) {
                self.typeHash = OrdersLib.EXACT_STANDING_ORDER_TYPEHASH;
            } else {
                // is flash order.
                self.typeHash = OrdersLib.EXACT_FLASH_ORDER_TYPEHASH;
            }
        }
    }

    function _hash(UserOrderBuffer memory self, OrderVariantMap variant) internal pure returns (bytes32 hashed) {
        uint256 structLength = variant.isStanding() ? STANDING_ORDER_BYTES : FLASH_ORDER_BYTES;
        assembly ("memory-safe") {
            hashed := keccak256(self, structLength)
        }
    }

    function hash712(UserOrderBuffer memory self, OrderVariantMap variant, TypedDataHasher typedHasher)
        internal
        pure
        returns (bytes32)
    {
        return typedHasher.hashTypedData(self._hash(variant));
    }

    function logBytes(UserOrderBuffer memory self, OrderVariantMap variant) internal pure {
        uint256 structLength = variant.isStanding() ? STANDING_ORDER_BYTES : FLASH_ORDER_BYTES;
        uint256 offset;
        assembly ("memory-safe") {
            offset := self
        }
        console.log("structLength: %s", structLength);
        consoleext.logMemWords(offset, offset + structLength);
    }

    function loadAndComputeQuantity(
        UserOrderBuffer memory self,
        CalldataReader reader,
        OrderVariantMap variant,
        PriceOutVsIn price,
        uint256 feeRay
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

        if (DEBUG_LOGS) console.log("price: %27e", PriceOutVsIn.unwrap(price));

        if (variant.exactIn() || variant.quantitiesPartial()) {
            quantityIn = AmountIn.wrap(quantity);
            if (DEBUG_LOGS) console.log("quantity in: %18e", AmountIn.unwrap(quantityIn));
            quantityOut = price.convert(quantityIn);
            if (DEBUG_LOGS) console.log("quantity out (pre fee): %18e", AmountOut.unwrap(quantityOut));
            quantityOut = quantityOut - quantityOut.mulRayScalar(feeRay);
            if (DEBUG_LOGS) console.log("quantity out (post fee): %18e", AmountOut.unwrap(quantityOut));
        } else {
            quantityOut = AmountOut.wrap(quantity);
            if (DEBUG_LOGS) console.log("quantity out: %18e", AmountOut.unwrap(quantityOut));
            quantityIn = price.convert(quantityOut);
            if (DEBUG_LOGS) console.log("quantity in (pre fee): %18e", AmountIn.unwrap(quantityIn));
            quantityIn = quantityIn + quantityIn.mulRayScalar(feeRay);
            if (DEBUG_LOGS) console.log("quantity in (post fee): %18e", AmountIn.unwrap(quantityIn));
        }

        return (reader, quantityIn, quantityOut);
    }

    function readOrderValidation(UserOrderBuffer memory self, CalldataReader reader, OrderVariantMap variant)
        internal
        view
        returns (CalldataReader)
    {
        if (variant.isStanding()) {
            // Copy slices directly from calldata into memory.
            assembly ("memory-safe") {
                calldatacopy(add(self, add(0x120, sub(0x20, 8))), reader, 8)
                calldatacopy(add(self, add(0x140, sub(0x20, 5))), add(reader, 8), 5)
                reader := add(reader, 13)
            }
        } else {
            // Nothing loaded from calldata, reader stays unmodified.
            self.nonce_or_validForBlock = uint64(block.number);
        }
        return reader;
    }
}
