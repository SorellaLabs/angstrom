// SPDX-License-Identifier: MIT
pragma solidity ^0.8.4;

import {Currency} from "v4-core/src/types/Currency.sol";
import {PoolKey} from "v4-core/src/types/PoolKey.sol";
import {IHooks} from "v4-core/src/interfaces/IHooks.sol";
import {TICK_SPACING} from "../Constants.sol";

/// @author philogy <https://github.com/philogy>
library ConversionLib {
    using ConversionLib for *;

    error Overflow();

    function intoC(address addr) internal pure returns (Currency) {
        return Currency.wrap(addr);
    }

    function toPoolKey(address hook, address asset0, address asset1) internal pure returns (PoolKey memory) {
        return PoolKey({
            currency0: intoC(asset0),
            currency1: intoC(asset1),
            fee: 0,
            tickSpacing: TICK_SPACING,
            hooks: IHooks(hook)
        });
    }

    function into(bool x) internal pure returns (uint256 y) {
        // forgefmt: disable-next-item
        assembly { y := x }
    }

    function into(address x) internal pure returns (uint256 y) {
        // forgefmt: disable-next-item
        assembly { y := x }
    }
}
