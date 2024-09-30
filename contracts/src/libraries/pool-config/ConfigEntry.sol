// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

/// @dev Packed `storeKey:u216 ++ tickSpacing:u16 ++ feeInE6:u24`
type ConfigEntry is uint256;

uint256 constant ENTRY_SIZE = 32;
uint256 constant KEY_MASK = 0xffffffffffffffffffffffffffffffffffffffffffffffffffffff0000000000;
uint256 constant TICK_SPACING_MASK = 0xffff;
uint256 constant TICK_SPACING_OFFSET = 24;
uint256 constant FEE_MASK = 0xffffff;
uint256 constant FEE_OFFSET = 0;

using ConfigEntryLib for ConfigEntry global;

/// @author philogy <https://github.com/philogy>
library ConfigEntryLib {
    function isEmpty(ConfigEntry self) internal pure returns (bool) {
        return ConfigEntry.unwrap(self) == 0;
    }

    function tickSpacing(ConfigEntry self) internal pure returns (int24 spacing) {
        assembly {
            spacing := and(TICK_SPACING_MASK, shr(TICK_SPACING_OFFSET, self))
        }
    }

    function feeInE6(ConfigEntry self) internal pure returns (uint24 fee) {
        return uint24(ConfigEntry.unwrap(self) >> FEE_OFFSET);
    }
}
