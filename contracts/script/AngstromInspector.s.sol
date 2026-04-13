// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {Script} from "forge-std/Script.sol";
import {console} from "forge-std/console.sol";

import {IAngstromAuth} from "src/interfaces/IAngstromAuth.sol";
import {IPoolManager} from "src/interfaces/IUniV4.sol";
import {AngstromInspector} from "src/periphery/AngstromInspector.sol";

/// @author crypto-banker <https://github.com/crypto-banker>
contract AngstromInspectorScript is Script {
    function run() public {
        address uniswap = uniswapOnCurrentChain();
        address angstrom = angstromOnCurrentChain();

        vm.startBroadcast();
        AngstromInspector inspector =
            new AngstromInspector(IAngstromAuth(angstrom), IPoolManager(uniswap));

        vm.stopBroadcast();
        console.log("[INFO] deployed AngstromInspector to %s", address(inspector));
    }

    function isChain(string memory name) internal returns (bool) {
        return getChain(name).chainId == block.chainid;
    }

    function uniswapOnCurrentChain() internal returns (address) {
        if (isChain("sepolia")) {
            return 0xE03A1074c86CFeDd5C142C4F04F1a1536e203543;
        }
        if (isChain("mainnet")) {
            return 0x000000000004444c5dc75cB358380D2e3dE08A90;
        }
        revert(string.concat("Unsupported chain: ", getChain(block.chainid).name));
    }

    function angstromOnCurrentChain() internal returns (address) {
        if (isChain("sepolia")) {
            return 0x9051085355BA7e36177e0a1c4082cb88C270ba90;
        }
        if (isChain("mainnet")) {
            return 0x0000000aa232009084Bd71A5797d089AA4Edfad4;
        }
        revert(string.concat("Unsupported chain: ", getChain(block.chainid).name));
    }
}
