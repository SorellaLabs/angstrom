// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {IPoolManager} from "../interfaces/IUniV4.sol";
import {Hooks, IHooks} from "v4-core/src/libraries/Hooks.sol";
import {Currency} from "v4-core/src/types/Currency.sol";
import {POOL_FEE} from "src/Constants.sol";

/// @author philogy <https://github.com/philogy>
abstract contract UniConsumer {
    using Hooks for IHooks;

    error NotUniswap();

    IPoolManager internal immutable UNI_V4;

    error InvalidHookPermissions();

    constructor(IPoolManager uniV4) {
        UNI_V4 = uniV4;
    }

    function _onlyUniV4() internal view {
        if (msg.sender != address(UNI_V4)) revert NotUniswap();
    }

    function _checkAngstromHookFlags() internal view {
        if (!hasAngstromHookFlags(address(this))) {
            revert InvalidHookPermissions();
        }
    }

    function _c(address addr) internal pure returns (Currency) {
        return Currency.wrap(addr);
    }
}

using Hooks for IHooks;

function hasAngstromHookFlags(address addr) pure returns (bool) {
    IHooks hook = IHooks(addr);

    // Need at least 1 of the flags to control initialization.
    if (!hook.hasPermission(Hooks.BEFORE_INITIALIZE_FLAG | Hooks.AFTER_INITIALIZE_FLAG)) {
        return false;
    }

    // Ensure that we exactly only enable before add & remove, no after hooks.
    if (!hook.hasPermission(Hooks.BEFORE_ADD_LIQUIDITY_FLAG)) return false;
    if (hook.hasPermission(Hooks.AFTER_ADD_LIQUIDITY_FLAG)) return false;
    if (!hook.hasPermission(Hooks.BEFORE_REMOVE_LIQUIDITY_FLAG)) return false;
    if (hook.hasPermission(Hooks.AFTER_REMOVE_LIQUIDITY_FLAG)) return false;

    // Ensure that we have some hook preventing 3rd party swapping.
    if (!hook.hasPermission(Hooks.BEFORE_SWAP_FLAG | Hooks.AFTER_SWAP_FLAG)) return false;

    return hook.isValidHookAddress(POOL_FEE);
}
