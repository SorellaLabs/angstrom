// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {UniConsumer} from "./UniConsumer.sol";
import {Hooks} from "v4-core/src/libraries/Hooks.sol";
import {PoolKey} from "v4-core/src/types/PoolKey.sol";

import {console} from "forge-std/console.sol";
import {POOL_FEE, TICK_SPACING} from "../Constants.sol";
import {IBeforeInitializeHook} from "../interfaces/IHooks.sol";

/// @author philogy <https://github.com/philogy>
abstract contract HookManager is UniConsumer, IBeforeInitializeHook {
    error InvalidPoolKey();

    constructor() {
        _checkHookPermissions(Hooks.BEFORE_SWAP_FLAG | Hooks.BEFORE_INITIALIZE_FLAG);
    }

    function beforeInitialize(address, PoolKey calldata poolKey, uint160, bytes calldata)
        external
        view
        onlyUniV4
        returns (bytes4)
    {
        if (poolKey.tickSpacing != TICK_SPACING || poolKey.fee != POOL_FEE) revert InvalidPoolKey();
        return this.beforeInitialize.selector;
    }
}
