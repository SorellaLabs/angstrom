// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {Test} from "forge-std/Test.sol";
import {Script} from "forge-std/Script.sol";
import {PoolGate} from "../test/_helpers/PoolGate.sol";
import {MockERC20} from "super-sol/mocks/MockERC20.sol";
import {PoolManager} from "v4-core/src/PoolManager.sol";

import {console2 as console} from "forge-std/console2.sol";

/// @author philogy <https://github.com/philogy>
contract TestnetSetupScript is Test, Script {
    // This is a known private key (default anvil account key), so it's fine to put it in plain text
    // here, *DO NOT* put an actual deployment key into plain text like this.
    uint256 internal constant TEST_ACCOUNT_PRIV_KEY7 =
        0x4bbbf85ce3377467afe5d46f804f221813b2bb87f24d81f60f1fcdbf7cbf4356;

    function run() public {
        uint256 key = TEST_ACCOUNT_PRIV_KEY7;

        vm.startBroadcast(key);

        PoolManager uniV4 = new PoolManager(address(0));
        console.log("Uniswap V4: %s", address(uniV4));

        PoolGate gate = new PoolGate(address(uniV4));

        console.log("Pool gate: %s", address(gate));

        MockERC20 token0 = new MockERC20();
        MockERC20 token1 = new MockERC20();

        console.log("(%s, %s)", address(token0), address(token1));

        vm.stopBroadcast();
    }
}
