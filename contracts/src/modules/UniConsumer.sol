// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {IPoolManager} from "../interfaces/IUniV4.sol";
import {IHooks} from "v4-core/src/interfaces/IHooks.sol";
import {Hooks} from "v4-core/src/libraries/Hooks.sol";

/// @author philogy <https://github.com/philogy>
abstract contract UniConsumer {
    using Hooks for IHooks;

    error NotUniswap();

    IPoolManager internal immutable UNI_V4;

    error MissingHookPermissions(uint160);

    modifier onlyUniV4() {
        if (msg.sender != address(UNI_V4)) revert NotUniswap();
        _;
    }

    constructor(address uniV4PoolManager) {
        UNI_V4 = IPoolManager(uniV4PoolManager);
    }

    function _checkHookPermissions(uint160 flags) internal view {
        if (uint160(address(this)) & flags != flags) revert MissingHookPermissions(flags);
    }
}
