// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "test/_helpers/BaseTest.sol";
import {UniV4Inspector} from "test/_view-ext/UniV4Inspector.sol";
import {OpenAngstrom} from "test/_mocks/OpenAngstrom.sol";
import {PoolGate} from "test/_helpers/PoolGate.sol";
import {MockERC20} from "super-sol/mocks/MockERC20.sol";
import {EnumerableSetLib} from "solady/src/utils/EnumerableSetLib.sol";

struct Env {
    address owner;
    address controller;
    address feeMaster;
    UniV4Inspector uniV4;
    OpenAngstrom angstrom;
    PoolGate gate;
}

/// @author philogy <https://github.com/philogy>
contract AngstromHandler is BaseTest {
    using EnumerableSetLib for *;

    Env e;

    EnumerableSetLib.AddressSet internal _actors;
    EnumerableSetLib.AddressSet internal _enabledAssets;

    mapping(address asset => uint256 total) public ghost_totalHeldBalance;

    constructor(Env memory env) {
        e = env;
    }

    function enabledAssets() public view returns (address[] memory) {
        return _enabledAssets.values();
    }

    function actors() public view returns (address[] memory) {
        return _actors.values();
    }
}
