// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "test/_helpers/BaseTest.sol";

import {UniV4Inspector} from "test/_view-ext/UniV4Inspector.sol";
import {OpenAngstrom} from "test/_mocks/OpenAngstrom.sol";
import {PoolGate} from "test/_helpers/PoolGate.sol";
import {MockERC20} from "super-sol/mocks/MockERC20.sol";

struct Env {
    address owner;
    address controller;
    address feeMaster;
    UniV4Inspector uniV4;
    OpenAngstrom angstrom;
    PoolGate gate;
}

/// @author philogy <https://github.com/philogy>
contract AngstromInvariantsTest is BaseTest {
    Env e;

    function setUp() public {
        e.owner = makeAddr("owner");
        e.controller = makeAddr("controller");
        e.feeMaster = makeAddr("feeMaster");

        vm.prank(e.owner);
        e.uniV4 = new UniV4Inspector();
        e.gate = new PoolGate(address(e.uniV4));
        e.angstrom = OpenAngstrom(
            deployAngstrom(type(OpenAngstrom).creationCode, e.uniV4, e.controller, e.feeMaster)
        );
    }
}
