// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

type TWAPOrderVariantMap is uint8;

using TWAPOrderVariantMapLib for TWAPOrderVariantMap global;

/// @author philogy <https://github.com/philogy>
library TWAPOrderVariantMapLib {
    uint256 internal constant USE_INTERNAL_BIT = 0x01;
    uint256 internal constant HAS_RECIPIENT_BIT = 0x02;
    uint256 internal constant HAS_HOOK_BIT = 0x04;
    uint256 internal constant ZERO_FOR_ONE_BIT = 0x08;
    uint256 internal constant IS_EXACT_IN_BIT = 0x10;
    uint256 internal constant IS_ECDSA_BIT = 0x20;

    function useInternal(TWAPOrderVariantMap variant) internal pure returns (bool) {
        return TWAPOrderVariantMap.unwrap(variant) & USE_INTERNAL_BIT != 0;
    }

    function recipientIsSome(TWAPOrderVariantMap variant) internal pure returns (bool) {
        return TWAPOrderVariantMap.unwrap(variant) & HAS_RECIPIENT_BIT != 0;
    }

    function noHook(TWAPOrderVariantMap variant) internal pure returns (bool) {
        return TWAPOrderVariantMap.unwrap(variant) & HAS_HOOK_BIT == 0;
    }

    function zeroForOne(TWAPOrderVariantMap variant) internal pure returns (bool) {
        return TWAPOrderVariantMap.unwrap(variant) & ZERO_FOR_ONE_BIT != 0;
    }

    function exactIn(TWAPOrderVariantMap variant) internal pure returns (bool) {
        return TWAPOrderVariantMap.unwrap(variant) & IS_EXACT_IN_BIT != 0;
    }

    function isEcdsa(TWAPOrderVariantMap variant) internal pure returns (bool) {
        return TWAPOrderVariantMap.unwrap(variant) & IS_ECDSA_BIT != 0;
    }
}
