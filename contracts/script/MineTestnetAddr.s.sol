// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseScript} from "./BaseScript.sol";
import {Angstrom} from "src/Angstrom.sol";
import {ControllerV1, IAngstromAuth} from "src/periphery/ControllerV1.sol";
import {TimelockController} from "@openzeppelin/contracts/governance/TimelockController.sol";
import {console} from "forge-std/console.sol";
import {hasAngstromHookFlags} from "src/modules/UniConsumer.sol";

/// @author philogy <https://github.com/philogy>
contract MineTestnetAddrScript is BaseScript {
    function run() public {
        uint256 pk = vm.envUint("TESTNET_PK");
        address owner = vm.addr(pk);
        vm.startBroadcast(pk);

        uint256 id = uint256(uint160(owner)) << 96;
        uint8 nonce = 0;
        while (!_foundAddr(id, nonce)) {
            nonce++;
            if (nonce > 32) {
                nonce = 0;
                id++;
            }
        }

        VANITY_MARKET.mint(owner, id, nonce);

        console.log("id: %x", id);
    }

    function _foundAddr(uint256 id, uint8 nonce) internal view returns (bool) {
        if (!hasAngstromHookFlags(VANITY_MARKET.computeAddress(bytes32(id), nonce))) return false;
        try VANITY_MARKET.addressOf(id) returns (address) {
            return false;
        } catch (bytes memory errData) {
            if (bytes4(errData) != bytes4(keccak256("TokenDoesNotExist()"))) {
                assembly ("memory-safe") {
                    revert(add(errData, 0x20), mload(errData))
                }
            }
            return true;
        }
    }
}
