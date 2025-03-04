// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {MockERC20} from "super-sol/mocks/MockERC20.sol";

contract MintableMockERC20 is MockERC20 {
    string internal _name = "Mock Token";
    string internal _symbol = "MCK";

    function setMeta(string memory newName, string memory newSymbol) external {
        _name = newName;
        _symbol = newSymbol;
    }

    function name() public view override returns (string memory) {
        return _name;
    }

    function symbol() public view override returns (string memory) {
        return _symbol;
    }
}
