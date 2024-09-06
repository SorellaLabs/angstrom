// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {IERC1271} from "./IERC1271.sol";

/// @author philogy <https://github.com/philogy>
contract RevertingERC1271 is IERC1271 {
    bytes revertData;

    function setRevertData(bytes calldata data) external {
        revertData = data;
    }

    function isValidSignature(bytes32, bytes calldata) external view returns (bytes4) {
        bytes memory data = revertData;
        assembly ("memory-safe") {
            revert(add(data, 0x20), mload(data))
        }
    }
}
