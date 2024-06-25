// SPDX-License-Identifier: GPL-2.0-or-later
pragma solidity ^0.8.21;

import {ERC20} from "solady/src/tokens/ERC20.sol";

contract MockERC20 is ERC20 {
    function name() public pure override returns (string memory) {
        return "TestERC20";
    }

    function symbol() public pure override returns (string memory) {
        return "TEST";
    }

    function mint(address to, uint256 amount) external {
        _mint(to, amount);
    }
}
