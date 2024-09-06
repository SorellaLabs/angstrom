// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

/// @dev Expected return magic (`keccak256("Angstrom.hook.return-magic")[-4:]`).
uint32 constant EXPECTED_HOOK_RETURN_MAGIC = 0x24a2e44b;

/// @author philogy <https://github.com/philogy>
interface IAngstromComposable {
    function compose(address from, bytes calldata payload) external returns (uint32);
}
