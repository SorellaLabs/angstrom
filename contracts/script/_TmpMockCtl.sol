// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

/// Stand-in for ControllerV1 with a zero fastOwner. No immutables, so the deployed bytecode can
/// be injected verbatim with anvil_setCode.
contract TmpMockCtl {
    function owner() external pure returns (address) {
        return address(0xA11CE);
    }

    function fastOwner() external pure returns (address) {
        return address(0);
    }

    function ANGSTROM() external pure returns (address) {
        return 0x0000000aa232009084Bd71A5797d089AA4Edfad4;
    }
}
