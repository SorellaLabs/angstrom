// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "test/_helpers/BaseTest.sol";
import {PoolId} from "v4-core/src/types/PoolId.sol";
import {Positions, Position} from "src/types/Positions.sol";
import {Position as UniPosition} from "v4-core/src/libraries/Position.sol";

/// @author philogy <https://github.com/philogy>
contract PositionsLibTest is BaseTest {
    Positions internal positions;

    function test_fuzzing_positionKey(
        PoolId id,
        address sender,
        int24 lowerTick,
        int24 upperTick,
        bytes32 salt
    ) public view {
        (, bytes32 positionKey) = positions.get(id, sender, lowerTick, upperTick, salt);
        assertEq(
            keccak256(abi.encodePacked(sender, lowerTick, upperTick, salt)),
            positionKey,
            "keccak256(...) != positions.get(...)"
        );
        assertEq(
            UniPosition.calculatePositionKey(sender, lowerTick, upperTick, salt),
            positionKey,
            "uni != angstrom"
        );
    }
}
