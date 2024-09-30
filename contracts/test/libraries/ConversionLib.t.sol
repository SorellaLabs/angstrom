// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "test/_helpers/BaseTest.sol";
import {ConversionLib} from "../../src/libraries/ConversionLib.sol";
import {PoolId, PoolIdLibrary} from "v4-core/src/types/PoolId.sol";
import {PoolKey} from "v4-core/src/types/PoolKey.sol";

/// @author philogy <https://github.com/philogy>
contract ConversionLibTest is BaseTest {
    function test_fuzzing_angstrom_uniV4_toId_equivalent(PoolKey calldata poolKey) public pure {
        bytes32 angId = PoolId.unwrap(ConversionLib.toId(poolKey));
        bytes32 v4Id = PoolId.unwrap(PoolIdLibrary.toId(poolKey));
        assertEq(angId, v4Id);
    }
}
