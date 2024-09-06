// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

/// @author philogy <https://github.com/philogy>
interface IERC1271 {
    function isValidSignature(bytes32, bytes calldata) external returns (bytes4);
}
