// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "test/_helpers/BaseTest.sol";
import {IPoolManager} from "v4-core/src/interfaces/IPoolManager.sol";
import {OpenAngstrom} from "test/_mocks/OpenAngstrom.sol";
import {Angstrom} from "src/Angstrom.sol";
import {PoolConfigStore} from "src/libraries/PoolConfigStore.sol";

/// @author philogy <https://github.com/philogy>
contract BaseTestTest is BaseTest {
    IPoolManager uni;
    address controller = makeAddr("controller");
    Angstrom real;
    OpenAngstrom open;

    function setUp() public {
        real = Angstrom(deployAngstrom(type(Angstrom).creationCode, uni, controller));
        open = OpenAngstrom(deployAngstrom(type(OpenAngstrom).creationCode, uni, controller));
    }

    function test_configStoreSlot() public {
        assertEq(
            rawGetConfigStore(address(real)),
            rawGetConfigStore(address(open)),
            "Default get config store mismatch real != open"
        );
        vm.startPrank(controller);
        real.configurePool(address(1), address(2), 1, 0);
        open.configurePool(address(1), address(2), 1, 0);
        vm.stopPrank();

        assertEq(
            rawGetConfigStore(address(real)).code,
            rawGetConfigStore(address(open)).code,
            "After set config store mismatch real != open"
        );

        assertEq(
            PoolConfigStore.unwrap(open.configStore()),
            rawGetConfigStore(address(open)),
            "open view method != raw get"
        );
    }
}
