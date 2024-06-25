// SPDX-License-Identifier: MIT
pragma solidity ^0.8.4;

import {Currency} from "v4-core/src/types/Currency.sol";
import {PoolKey} from "v4-core/src/types/PoolKey.sol";
import {BitmapLib} from "./BitmapLib.sol";
import {IHooks} from "v4-core/src/interfaces/IHooks.sol";

/// @author philogy <https://github.com/philogy>
library ConversionLib {
    /// TODO: Determine
    int24 internal constant TICK_SPACING = 60;

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

    function toTick(int16 wordPos, uint8 bitPos) internal pure returns (int24) {
        return (int24(wordPos) * 256 + int24(uint24(bitPos))) * TICK_SPACING;
    }

    function neg(uint256 x) internal pure returns (int256) {
        if (x > (1 << 255)) revert Overflow();
        unchecked {
            return int256(0 - x);
        }
    }

    function pos(uint256 x) internal pure returns (int256 y) {
        if (x >= (1 << 255)) revert Overflow();
        return int256(x);
    }

    function into(bool x) internal pure returns (uint256 y) {
        /// @solidity memory-safe-assembly
        assembly {
            y := x
        }
    }

    function into(address x) internal pure returns (uint256 y) {
        /// @solidity memory-safe-assembly
        assembly {
            y := x
        }
    }
}
