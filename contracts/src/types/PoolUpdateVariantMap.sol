// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

type PoolUpdateVariantMap is uint8;

using PoolUpdateVariantMapLib for PoolUpdateVariantMap global;

/// @author philogy <https://github.com/philogy>
library PoolUpdateVariantMapLib {
    uint256 internal constant ZERO_FOR_ONE_FLAG = 0x01;
    uint256 internal constant CURRENTY_ONLY_FLAG = 0x02;

    function zeroForOne(PoolUpdateVariantMap self) internal pure returns (bool) {
        return PoolUpdateVariantMap.unwrap(self) & ZERO_FOR_ONE_FLAG != 0;
    }

    function currentOnly(PoolUpdateVariantMap self) internal pure returns (bool) {
        return PoolUpdateVariantMap.unwrap(self) & CURRENTY_ONLY_FLAG != 0;
    }
}
