// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {IPoolManager} from "../interfaces/IUniV4.sol";
import {Hooks} from "v4-core/src/libraries/Hooks.sol";
import {Currency} from "v4-core/src/types/Currency.sol";
import {ANGSTROM_HOOK_FLAGS} from "src/Constants.sol";

/// @author philogy <https://github.com/philogy>
abstract contract UniConsumer {
    error NotUniswap();

    IPoolManager internal immutable UNI_V4;

    error MissingHookPermissions();

    constructor(IPoolManager uniV4) {
        UNI_V4 = uniV4;
    }

    function _onlyUniV4() internal view {
        if (msg.sender != address(UNI_V4)) revert NotUniswap();
    }

    function _checkAngstromHookFlags() internal view {
        if (uint160(address(this)) & Hooks.ALL_HOOK_MASK != ANGSTROM_HOOK_FLAGS) {
            revert MissingHookPermissions();
        }
    }

    function _c(address addr) internal pure returns (Currency) {
        return Currency.wrap(addr);
    }
}
