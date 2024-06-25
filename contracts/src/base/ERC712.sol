// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {EIP712} from "solady/src/utils/EIP712.sol";

/// @author philogy <https://github.com/philogy>
abstract contract ERC712 is EIP712 {
    function _domainNameAndVersion() internal pure override returns (string memory, string memory) {
        return ("Angstrom", "v1");
    }
}
