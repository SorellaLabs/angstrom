// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

type OrderVariantMap is uint8;

using OrderVariantMapLib for OrderVariantMap global;

/// @author philogy <https://github.com/philogy>
library OrderVariantMapLib {
    uint256 internal constant USE_INTERNAL_BIT = 0x01;
    uint256 internal constant HAS_RECIPIENT_BIT = 0x02;
    uint256 internal constant HAS_HOOK_BIT = 0x04;
    uint256 internal constant A_TO_B_BIT = 0x08;
    uint256 internal constant IS_STANDING_BIT = 0x10;
    uint256 internal constant QTY_PARTIAL_BIT = 0x20;
    uint256 internal constant IS_EXACT_IN_BIT = 0x40;
    uint256 internal constant IS_ECDSA_BIT = 0x80;

    function useInternal(OrderVariantMap variant) internal pure returns (bool) {
        return OrderVariantMap.unwrap(variant) & USE_INTERNAL_BIT != 0;
    }

    function recipientIsSome(OrderVariantMap variant) internal pure returns (bool) {
        return OrderVariantMap.unwrap(variant) & HAS_RECIPIENT_BIT != 0;
    }

    function noHook(OrderVariantMap variant) internal pure returns (bool) {
        return OrderVariantMap.unwrap(variant) & HAS_HOOK_BIT == 0;
    }

    function aToB(OrderVariantMap variant) internal pure returns (bool) {
        return OrderVariantMap.unwrap(variant) & A_TO_B_BIT != 0;
    }

    function isStanding(OrderVariantMap variant) internal pure returns (bool) {
        return OrderVariantMap.unwrap(variant) & IS_STANDING_BIT != 0;
    }

    function quantitiesPartial(OrderVariantMap variant) internal pure returns (bool) {
        return OrderVariantMap.unwrap(variant) & QTY_PARTIAL_BIT != 0;
    }

    function exactIn(OrderVariantMap variant) internal pure returns (bool) {
        return OrderVariantMap.unwrap(variant) & IS_EXACT_IN_BIT != 0;
    }

    function isEcdsa(OrderVariantMap variant) internal pure returns (bool) {
        return OrderVariantMap.unwrap(variant) & IS_ECDSA_BIT != 0;
    }

    function asB32(OrderVariantMap variant) internal pure returns (bytes32) {
        if (variant.isStanding()) {
            if (variant.quantitiesPartial()) return "Standing_Partial";
            else return "Standing_Exact";
        } else {
            if (variant.quantitiesPartial()) return "Flash_Partial";
            else return "Flash_Exact";
        }
    }
}
