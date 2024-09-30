// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "test/_helpers/BaseTest.sol";
import {IPoolManager} from "v4-core/src/interfaces/IPoolManager.sol";
import {ExtAngstrom} from "test/_view-ext/ExtAngstrom.sol";
import {Angstrom} from "src/Angstrom.sol";
import {PoolConfigStore} from "src/libraries/pool-config/PoolConfigStore.sol";

/// @author philogy <https://github.com/philogy>
contract BaseTestTest is BaseTest {
    IPoolManager uni;
    address controller = makeAddr("controller");
    Angstrom real;
    ExtAngstrom ext;

    function setUp() public {
        real = Angstrom(deployAngstrom(type(Angstrom).creationCode, uni, controller));
        ext = ExtAngstrom(deployAngstrom(type(ExtAngstrom).creationCode, uni, controller));
    }

    function test_configStoreSlot() public {
        assertEq(
            rawGetConfigStore(address(real)),
            rawGetConfigStore(address(ext)),
            "Default get config store mismatch real != ext"
        );
        vm.startPrank(controller);
        real.configurePool(address(1), address(2), 1, 0);
        ext.configurePool(address(1), address(2), 1, 0);
        vm.stopPrank();

        assertEq(
            rawGetConfigStore(address(real)).code,
            rawGetConfigStore(address(ext)).code,
            "After set config store mismatch real != ext"
        );

        assertEq(
            PoolConfigStore.unwrap(ext.configStore()),
            rawGetConfigStore(address(ext)),
            "Ext view method != raw get"
        );
    }
}
