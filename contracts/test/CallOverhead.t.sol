// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {Test} from "forge-std/Test.sol";
import {PoolManager} from "v4-core/src/PoolManager.sol";
import {PoolId} from "v4-core/src/types/PoolId.sol";

/// @author philogy <https://github.com/philogy>
contract CallOverheadTest is Test {
    PoolManager v4 = new PoolManager(0);

    function test_getTickInfo() public {
        PoolId id = PoolId.wrap(bytes32(0));
        // v4.getPoolTickInfo(id, 0);
        // v4.getPoolTickInfo(id, 0);
        // v4.getPoolTickInfo(id, 0);
    }
}
