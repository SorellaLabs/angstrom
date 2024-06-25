// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {Test} from "forge-std/Test.sol";
import {Script} from "forge-std/Script.sol";
import {Hooks, IHooks} from "v4-core/src/libraries/Hooks.sol";
import {Angstrom} from "../src/Angstrom.sol";
import {SEPOLIA_POOL_MANAGER_INITCODE} from "./SepoliaPoolManager.sol";
import {TestnetHub} from "../test/testnet-utils/TestnetHub.sol";
import {MockERC20} from "../test/mocks/MockERC20.sol";

import {console2 as console} from "forge-std/console2.sol";

/// @author philogy <https://github.com/philogy>
contract TestnetDeploy is Test, Script {
    using Hooks for IHooks;

    // This is a known private key (default anvil account key), so it's fine to put it in plain text
    // here, *DO NOT* put the actual deployment key into plain text like this.
    uint256 internal constant TEST_ACCOUNT_PRIV_KEY7 =
        0x4bbbf85ce3377467afe5d46f804f221813b2bb87f24d81f60f1fcdbf7cbf4356;

    function run() public {
        uint256 key = TEST_ACCOUNT_PRIV_KEY7;
        address deployer = vm.addr(key);

        vm.startBroadcast(key);

        address uniV4;

        bytes memory uniV4Initcode = SEPOLIA_POOL_MANAGER_INITCODE;

        assembly ("memory-safe") {
            uniV4 := create(0, add(uniV4Initcode, 0x20), mload(uniV4Initcode))
        }

        assertTrue(uniV4 != address(0), "Uniswap V4 deployment failed");
        console.log("Uniswap V4: %s", address(uniV4));

        address governance = deployer;

        bytes memory angstromInitcode = abi.encodePacked(type(Angstrom).creationCode, abi.encode(uniV4, governance));
        bytes32 initcodeHash = keccak256(angstromInitcode);

        bytes32 salt = bytes32(mineSalt(initcodeHash, Hooks.BEFORE_SWAP_FLAG));

        (bool success, bytes memory ret) = CREATE2_FACTORY.call(abi.encodePacked(salt, angstromInitcode));
        assertTrue(success);
        address angstrom = address(bytes20(ret));
        console.log("Angstrom: %s", angstrom);

        TestnetHub hub = new TestnetHub(angstrom, uniV4);

        console.log("Testnet hub: %s", address(hub));

        MockERC20 token0 = new MockERC20();
        MockERC20 token1 = new MockERC20();

        console.log("(%s, %s)", address(token0), address(token1));

        vm.stopBroadcast();
    }

    function mineSalt(bytes32 initcodeHash, uint160 flags) internal pure returns (uint256 salt) {
        while (uint160(computeCreate2Address(bytes32(salt), initcodeHash)) & Hooks.ALL_HOOK_MASK != flags) {
            salt++;
        }
    }
}
