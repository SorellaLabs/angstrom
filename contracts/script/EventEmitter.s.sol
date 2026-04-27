// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {Script} from "forge-std/Script.sol";
import {console} from "forge-std/console.sol";
import {EventEmitter} from "src/periphery/EventEmitter.sol";

contract EventEmitterScript is Script {
    function run() public {
        vm.startBroadcast();
        address timelock = timelockOnCurrentChain();

        EventEmitter emitter = new EventEmitter(timelock);
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
}
