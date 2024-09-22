// SPDX-License-Identifier: BUSL-1.1
pragma solidity ^0.8.13;

import {PoolId} from "v4-core/src/types/PoolId.sol";
import {POSITIONS_STORAGE_PREFIX} from "src/Constants.sol";

/// @dev Custom `(PoolId id, address owner, int24 lowerTick, int24 upperTick, bytes32 salt) => Position` mapping
struct Positions {
    uint256 __placeholder;
}

struct Position {
    uint256 pastRewards;
}

using PositionsLib for Positions global;

/// @author philogy <https://github.com/philogy>
library PositionsLib {
    function get(Positions storage positions, PoolId id, address sender, int24 lowerTick, int24 upperTick, bytes32 salt)
        internal
        pure
        returns (Position storage position, bytes32 positionKey)
    {
        assembly ("memory-safe") {
            let free := mload(0x40)

            // Compute `positionKey` as `keccak256(abi.encodePacked(sender, lowerTick, upperTick, sender))`.
            mstore(0x06, upperTick)
            mstore(0x03, lowerTick)
            mstore(0x00, sender)
            // WARN: Free memory pointer temporarily invalid from here on.
            mstore(0x26, salt)
            positionKey := keccak256(12, add(add(20, 3), add(3, 32)))

            mstore(0x00, id)
            mstore(0x20, positionKey)
            mstore(0x40, positions.slot)

            position.slot := or(shr(8, keccak256(0x00, 0x60)), shl(248, POSITIONS_STORAGE_PREFIX))

            mstore(0x40, free)
        }
    }
}
