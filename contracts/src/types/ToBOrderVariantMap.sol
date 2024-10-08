// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

type ToBOrderVariantMap is uint8;

using ToBOrderVariantMapLib for ToBOrderVariantMap global;

/// @author philogy <https://github.com/philogy>
library ToBOrderVariantMapLib {
    uint256 internal constant USE_INTERNAL_BIT = 0x01;
    uint256 internal constant ZERO_FOR_ONE_BIT = 0x02;
    uint256 internal constant HAS_RECIPIENT_BIT = 0x04;
    uint256 internal constant IS_ECDSA_BIT = 0x08;

    function useInternal(ToBOrderVariantMap variant) internal pure returns (bool) {
        return ToBOrderVariantMap.unwrap(variant) & USE_INTERNAL_BIT != 0;
    }

    function zeroForOne(ToBOrderVariantMap variant) internal pure returns (bool) {
        return ToBOrderVariantMap.unwrap(variant) & ZERO_FOR_ONE_BIT != 0;
    }

    function recipientIsSome(ToBOrderVariantMap variant) internal pure returns (bool) {
        return ToBOrderVariantMap.unwrap(variant) & HAS_RECIPIENT_BIT != 0;
    }

    function isEcdsa(ToBOrderVariantMap variant) internal pure returns (bool) {
        return ToBOrderVariantMap.unwrap(variant) & IS_ECDSA_BIT != 0;
    }
}
