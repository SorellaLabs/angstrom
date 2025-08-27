// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {IPoolManager} from "../interfaces/IUniV4.sol";
import {Hooks, IHooks} from "v4-core/src/libraries/Hooks.sol";
import {PoolKey} from "v4-core/src/types/PoolKey.sol";
import {PoolId} from "v4-core/src/types/PoolId.sol";
import {Currency} from "v4-core/src/types/Currency.sol";
import {LPFeeLibrary} from "v4-core/src/libraries/LPFeeLibrary.sol";

uint24 constant ANGSTROM_INIT_HOOK_FEE = LPFeeLibrary.DYNAMIC_FEE_FLAG;

/// @author philogy <https://github.com/philogy>
abstract contract UniConsumer {
    using Hooks for IHooks;

    error NotUniswap();

    IPoolManager internal immutable UNI_V4;

    uint24 internal constant INIT_HOOK_FEE = ANGSTROM_INIT_HOOK_FEE;

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

    function _addr(Currency c) internal pure returns (address) {
        return Currency.unwrap(c);
    }

    function _toId(PoolKey calldata poolKey) internal pure returns (PoolId id) {
        assembly ("memory-safe") {
            let ptr := mload(0x40)
            calldatacopy(ptr, poolKey, mul(32, 5))
            id := keccak256(ptr, mul(32, 5))
        }
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
    if (!hook.hasPermission(Hooks.BEFORE_SWAP_FLAG)) return false;
    if (
        !(
            hook.hasPermission(Hooks.AFTER_SWAP_FLAG)
                && hook.hasPermission(Hooks.AFTER_SWAP_RETURNS_DELTA_FLAG)
        )
    ) return false;

    return hook.isValidHookAddress(ANGSTROM_INIT_HOOK_FEE);
}
