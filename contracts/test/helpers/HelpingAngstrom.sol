// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Angstrom} from "../../src/Angstrom.sol";

/// @author philogy <https://github.com/philogy>
contract HelpingAngstrom is Angstrom {
    constructor(address uniV4PoolManager, address governance) Angstrom(uniV4PoolManager, governance) {}

    function hashTyped(bytes32 structHash) external view returns (bytes32) {
        return _hashTypedData(structHash);
    }
}
