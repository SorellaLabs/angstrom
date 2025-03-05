// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "test/_helpers/BaseTest.sol";
import {Script} from "forge-std/Script.sol";
import {Angstrom} from "src/Angstrom.sol";
import {ControllerV1, IAngstromAuth} from "src/periphery/ControllerV1.sol";
import {TimelockController} from "@openzeppelin/contracts/governance/TimelockController.sol";

/// @author philogy <https://github.com/philogy>
contract AngstromScript is BaseTest, Script {
    uint256 constant TIMELOCK_DELAY = 2 weeks;
    IVanityMarket constant VANITY_MARKET = IVanityMarket(0x000000000000b361194cfe6312EE3210d53C15AA);

    function run() public {
        vm.startBroadcast();

        address angstromMultisig = vm.envAddress("ANGSTROM_MULTISIG");
        uint256 angstromAddressTokenId = vm.envUint("ANGSTROM_ADDRESS_TOKEN_ID");
        address angstromAddress = VANITY_MARKET.addressOf(angstromAddressTokenId);
        address uniswap = vm.envAddress("V4_POOL_MANAGER");

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
        ControllerV1 controller = new ControllerV1(
            IAngstromAuth(angstromAddress), address(timelock), block.chainid == 11155111
        );
        VANITY_MARKET.deploy(
            angstromAddressTokenId,
            bytes.concat(type(Angstrom).creationCode, abi.encode(uniswap, controller))
        );
    }
}

interface IVanityMarket {
    function deploy(uint256 id, bytes calldata initcode)
        external
        payable
        returns (address deployed);

    function addressOf(uint256 id) external view returns (address vanity);
}
