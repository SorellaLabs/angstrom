// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {IERC1271} from "./IERC1271.sol";

/// @author philogy <https://github.com/philogy>
contract ReturnerERC1271 is IERC1271 {
    bytes4 retVal;

    function setReturnValue(bytes4 value) external {
        retVal = value;
    }

    function isValidSignature(bytes32, bytes calldata) external view returns (bytes4) {
        return retVal;
    }
}
