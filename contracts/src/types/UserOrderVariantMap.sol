// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import {LibString} from "solady/src/utils/LibString.sol";

type UserOrderVariantMap is uint8;

using UserOrderVariantMapLib for UserOrderVariantMap global;

/// @author philogy <https://github.com/philogy>
library UserOrderVariantMapLib {
    uint256 internal constant USE_INTERNAL_BIT = 0x01;
    uint256 internal constant HAS_RECIPIENT_BIT = 0x02;
    uint256 internal constant HAS_HOOK_BIT = 0x04;
    uint256 internal constant ZERO_FOR_ONE_BIT = 0x08;
    uint256 internal constant IS_STANDING_BIT = 0x10;
    uint256 internal constant QTY_PARTIAL_BIT = 0x20;
    uint256 internal constant IS_EXACT_IN_BIT = 0x40;
    uint256 internal constant IS_ECDSA_BIT = 0x80;

    uint256 internal constant SPECIFYING_IN_MASK = QTY_PARTIAL_BIT | IS_EXACT_IN_BIT;

    function useInternal(UserOrderVariantMap variant) internal pure returns (bool) {
        return UserOrderVariantMap.unwrap(variant) & USE_INTERNAL_BIT != 0;
    }

    function recipientIsSome(UserOrderVariantMap variant) internal pure returns (bool) {
        return UserOrderVariantMap.unwrap(variant) & HAS_RECIPIENT_BIT != 0;
    }

    function noHook(UserOrderVariantMap variant) internal pure returns (bool) {
        return UserOrderVariantMap.unwrap(variant) & HAS_HOOK_BIT == 0;
    }

    function zeroForOne(UserOrderVariantMap variant) internal pure returns (bool) {
        return UserOrderVariantMap.unwrap(variant) & ZERO_FOR_ONE_BIT != 0;
    }

    function specifyingInput(UserOrderVariantMap variant) internal pure returns (bool) {
        return UserOrderVariantMap.unwrap(variant) & SPECIFYING_IN_MASK != 0;
    }

    function isStanding(UserOrderVariantMap variant) internal pure returns (bool) {
        return UserOrderVariantMap.unwrap(variant) & IS_STANDING_BIT != 0;
    }

    function quantitiesPartial(UserOrderVariantMap variant) internal pure returns (bool) {
        return UserOrderVariantMap.unwrap(variant) & QTY_PARTIAL_BIT != 0;
    }

    function exactIn(UserOrderVariantMap variant) internal pure returns (bool) {
        return UserOrderVariantMap.unwrap(variant) & IS_EXACT_IN_BIT != 0;
    }

    function isEcdsa(UserOrderVariantMap variant) internal pure returns (bool) {
        return UserOrderVariantMap.unwrap(variant) & IS_ECDSA_BIT != 0;
    }

    function asB32(UserOrderVariantMap variant) internal pure returns (bytes32) {
        if (variant.isStanding()) {
            if (variant.quantitiesPartial()) return "Standing_Partial";
            else return "Standing_Exact";
        } else {
            if (variant.quantitiesPartial()) return "Flash_Partial";
            else return "Flash_Exact";
        }
    }

    function toStr(UserOrderVariantMap variant) internal pure returns (string memory) {
        return LibString.fromSmallString(variant.asB32());
    }
}
