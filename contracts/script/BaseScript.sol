// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "test/_helpers/BaseTest.sol";
import {Script} from "forge-std/Script.sol";

abstract contract BaseScript is BaseTest {
    IVanityMarket constant VANITY_MARKET = IVanityMarket(0x000000000000b361194cfe6312EE3210d53C15AA);
}

interface IVanityMarket {
    function mint(address to, uint256 id, uint8 nonce) external;

    function deploy(uint256 id, bytes calldata initcode)
        external
        payable
        returns (address deployed);

    function addressOf(uint256 id) external view returns (address vanity);

    function computeAddress(bytes32 salt, uint8 nonce) external view returns (address vanity);
}
