// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {MockERC20} from "super-sol/mocks/MockERC20.sol";
import {IUniV4, IPoolManager} from "src/interfaces/IUniV4.sol";
import {Currency} from "v4-core/src/types/Currency.sol";
import {CommonBase} from "forge-std/Base.sol";

/// @author philogy <https://github.com/philogy>
contract DeltaClearerERC20 is MockERC20, CommonBase {
    using IUniV4 for IPoolManager;

    IPoolManager immutable UNI_V4;
    address immutable ANGSTROM;

    address[] tokensToClear;

    constructor(address angstrom, IPoolManager uni) {
        ANGSTROM = angstrom;
        UNI_V4 = uni;
        require(
            address(vm).code.length > 0,
            "This contract uses vm.prank but vm not found, probably in prod-like environment"
        );
    }

    function addClear(address token) public {
        tokensToClear.push(token);
    }

    function transfer(address to, uint256 amount) public override returns (bool) {
        if (msg.sender == ANGSTROM) {
            for (uint256 i = 0; i < tokensToClear.length; i++) {
                address asset = tokensToClear[i];
                int256 delta = UNI_V4.getDelta(msg.sender, asset);
                if (delta > 0) {
                    vm.prank(msg.sender);
                    UNI_V4.clear(Currency.wrap(asset), uint256(delta));
                }
            }
        }
        return super.transfer(to, amount);
    }
}
