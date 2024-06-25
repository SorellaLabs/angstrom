// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

/// @author philogy <https://github.com/philogy>
interface IAngstromComposable {
    function compose(address from, bytes calldata payload) external returns (uint32);
}
