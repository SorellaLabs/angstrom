// SPDX-License-Identifier: BUSL-1.1
pragma solidity ^0.8.13;

import {PoolId} from "v4-core/src/types/PoolId.sol";

struct Positions {
    mapping(PoolId id => mapping(bytes32 uniPositionKey => Position)) positions;
}

struct Position {
    uint256 pastRewards;
}

using PositionsLib for Positions global;

/// @author philogy <https://github.com/philogy>
library PositionsLib {
    function get(
        Positions storage self,
        PoolId id,
        address owner,
        int24 lowerTick,
        int24 upperTick,
        bytes32 salt
    ) internal view returns (Position storage position, bytes32 positionKey) {
        assembly ("memory-safe") {
            let free := mload(0x40)

            // Compute `positionKey` as `keccak256(abi.encodePacked(owner, lowerTick, upperTick, salt))`.
            // Less efficient than alternative ordering *but* lets us reuse as Uniswap position key.
            mstore(0x06, upperTick)
            mstore(0x03, lowerTick)
            mstore(0x00, owner)
            // WARN: Free memory pointer temporarily invalid from here on.
            mstore(0x26, salt)
            positionKey := keccak256(12, add(add(3, 3), add(20, 32)))
            // Upper bytes of free memory pointer cleared.
            mstore(0x26, 0)
        }
        position = self.positions[id][positionKey];
    }
}
