// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseScript} from "./BaseScript.sol";
import {Angstrom} from "src/Angstrom.sol";
import {ControllerV1, IAngstromAuth} from "src/periphery/ControllerV1.sol";
import {TimelockController} from "@openzeppelin/contracts/governance/TimelockController.sol";
import {console} from "forge-std/console.sol";
import {hasAngstromHookFlags} from "src/modules/UniConsumer.sol";

/// @author philogy <https://github.com/philogy>
contract AngstromScript is BaseScript {
    uint256 constant TIMELOCK_DELAY = 2 weeks;

    function run() public {
        vm.startBroadcast(vm.envUint("TESTNET_PK"));

        uint256 angstromAddressTokenId = vm.envUint("ANGSTROM_ADDRESS_TOKEN_ID");
        address angstromAddress = VANITY_MARKET.addressOf(angstromAddressTokenId);

        require(
            hasAngstromHookFlags(VANITY_MARKET.addressOf(angstromAddressTokenId)), "Bad address"
        );

        address uniswap = uniswapOnCurrentChain();

        address controllerOwner;
        if (isTestnet()) {
            console.log("[INFO] Testnet detected, deploying *WITHOUT* timelock");
            controllerOwner = vm.envAddress("TESTNET_OWNER");
        } else {
            address angstromMultisig = vm.envAddress("ANGSTROM_MULTISIG");

            console.log("[INFO] Mainnet detected, deploying with timelock");
            // Allow anyone to execute
            address[] memory executors = new address[](1);
            executors[0] = address(0);

            // Only allow multisig to propose & cancel transactions.
            address[] memory proposers = new address[](1);
            proposers[0] = angstromMultisig;

            TimelockController timelock = new TimelockController({
                minDelay: TIMELOCK_DELAY,
                proposers: proposers,
                executors: executors,
                admin: address(0)
            });
            controllerOwner = address(timelock);
        }

        ControllerV1 controller = new ControllerV1(IAngstromAuth(angstromAddress), controllerOwner);
        VANITY_MARKET.deploy(
            angstromAddressTokenId,
            bytes.concat(type(Angstrom).creationCode, abi.encode(uniswap, controller))
        );

        console.log("angstrom: %s", angstromAddress);
        console.log("controller: %s", address(controller));
    }

    function isTestnet() internal returns (bool) {
        return isChain("sepolia") || isChain("holesky");
    }

    function isChain(string memory name) internal returns (bool) {
        return getChain(name).chainId == block.chainid;
    }

    function uniswapOnCurrentChain() internal returns (address) {
        if (isChain("sepolia")) return 0xE03A1074c86CFeDd5C142C4F04F1a1536e203543;
        if (isChain("mainnet")) return 0x000000000004444c5dc75cB358380D2e3dE08A90;
        revert(string.concat("Unsupported chain: ", getChain(block.chainid).name));
    }
}
