// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Angstrom} from "../../src/Angstrom.sol";
import {SafeCastLib} from "solady/src/utils/SafeCastLib.sol";

/// @author philogy <https://github.com/philogy>
contract ExtAngstrom is Angstrom {
    constructor(address uniV4PoolManager, address governance) Angstrom(uniV4PoolManager, governance) {}

    // TODO: Remove
    function __ilegalMint(address to, address asset, uint256 amount) external {
        _angstromReserves[to][asset] += amount;
    }

    function updateLastBlock() public {
        lastBlockUpdated = SafeCastLib.toUint64(block.number);
    }

    function isNode(address addr) public view returns (bool) {
        return _isNode[addr];
    }

    function DOMAIN_SEPARATOR() external view returns (bytes32) {
        return _domainSeparator();
    }

    function hashTyped(bytes32 structHash) external view returns (bytes32) {
        return _hashTypedData(structHash);
    }
}
