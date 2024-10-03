// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {Hooks} from "v4-core/src/libraries/Hooks.sol";

uint24 constant POOL_FEE = 0;

uint160 constant ANGSTROM_HOOK_FLAGS = Hooks.BEFORE_SWAP_FLAG | Hooks.BEFORE_INITIALIZE_FLAG
    | Hooks.BEFORE_ADD_LIQUIDITY_FLAG | Hooks.BEFORE_REMOVE_LIQUIDITY_FLAG;
