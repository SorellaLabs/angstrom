// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {Script} from "forge-std/Script.sol";
import {console} from "forge-std/console.sol";
import {EventEmitter} from "src/periphery/EventEmitter.sol";

contract EventEmitterScript is Script {
    function run() public {
        address timelock = timelockOnCurrentChain();
        address multisig = multisigOnCurrentChain();

        address[] memory initialAdmins = new address[](2);
        initialAdmins[0] = timelock;
        initialAdmins[1] = multisig;

        vm.startBroadcast();
        EventEmitter emitter = new EventEmitter(initialAdmins);
        vm.stopBroadcast();

        console.log("[INFO] deployed event emitter to %s", address(emitter));
    }

    function isChain(string memory name) internal returns (bool) {
        return getChain(name).chainId == block.chainid;
    }

    function timelockOnCurrentChain() internal returns (address) {
        if (isChain("mainnet")) {
            return 0x60D41d9708BBEfd29000d1486C6406Ef23526c01;
        }
        revert(string.concat("Unsupported chain: ", getChain(block.chainid).name));
    }

    function multisigOnCurrentChain() internal returns (address) {
        if (isChain("mainnet")) {
            return 0xD31C82069da3013fdB16B731AD19076Af9b93105;
        }
        revert(string.concat("Unsupported chain: ", getChain(block.chainid).name));
    }
}
