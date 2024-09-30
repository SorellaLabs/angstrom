// SPDX-License-Identifier: MIT
pragma solidity ^0.8.4;

import {Currency} from "v4-core/src/types/Currency.sol";
import {PoolKey} from "v4-core/src/types/PoolKey.sol";
import {PoolId} from "v4-core/src/types/PoolId.sol";
import {IHooks} from "v4-core/src/interfaces/IHooks.sol";

/// @author philogy <https://github.com/philogy>
library ConversionLib {
    using ConversionLib for *;

    function intoC(address addr) internal pure returns (Currency) {
        return Currency.wrap(addr);
    }

    function toPoolKey(address hook, address asset0, address asset1, int24 tickSpacing)
        internal
        pure
        returns (PoolKey memory)
    {
        return PoolKey({
            currency0: intoC(asset0),
            currency1: intoC(asset1),
            fee: 0,
            tickSpacing: tickSpacing,
            hooks: IHooks(hook)
        });
    }

    function toId(PoolKey calldata poolKey) internal pure returns (PoolId id) {
        assembly ("memory-safe") {
            let ptr := mload(0x40)
            calldatacopy(ptr, poolKey, mul(32, 5))
            id := keccak256(ptr, mul(32, 5))
        }
    }
}
