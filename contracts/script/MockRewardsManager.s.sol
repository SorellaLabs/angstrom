// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {Test} from "forge-std/Test.sol";
import {Script} from "forge-std/Script.sol";
import {HookDeployer} from "../test/_helpers/HookDeployer.sol";
import {MockRewardsManager} from "../test/_mocks/MockRewardsManager.sol";
import {ANGSTROM_HOOK_FLAGS} from "src/Constants.sol";

import {console2 as console} from "forge-std/console2.sol";

/// @author philogy <https://github.com/philogy>
contract MockRewardsManagerScript is Test, Script, HookDeployer {
    // This is a known private key (default anvil account key), so it's fine to put it in plain text
    // here, *DO NOT* put an actual deployment key into plain text like this.
    uint256 internal constant TEST_ACCOUNT_PRIV_KEY7 =
        0x4bbbf85ce3377467afe5d46f804f221813b2bb87f24d81f60f1fcdbf7cbf4356;

    function run() public {
        uint256 key = TEST_ACCOUNT_PRIV_KEY7;

        address UNI_V4_PM = vm.envAddress("V4_POOL_MANAGER");

        vm.startBroadcast(key);

        (bool suc, address mockRewardsAddr,) = deployHook(
            abi.encodePacked(type(MockRewardsManager).creationCode, abi.encode(UNI_V4_PM)),
            ANGSTROM_HOOK_FLAGS,
            CREATE2_FACTORY
        );

        assertTrue(suc);

        console.log("mockRewardsAddr: %s", mockRewardsAddr);

        vm.stopBroadcast();
    }
}
