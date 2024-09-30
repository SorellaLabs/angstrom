// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import {MixedSignLib} from "../libraries/MixedSignLib.sol";
import {tint256} from "transient-goodies/TransientPrimitives.sol";
import {BalanceDelta} from "v4-core/src/types/BalanceDelta.sol";

struct DeltaTracker {
    mapping(address asset => tint256 netBalances) deltas;
}

using DeltaTrackerLib for DeltaTracker global;

/// @author philogy <https://github.com/philogy>
library DeltaTrackerLib {
    function add(DeltaTracker storage self, address asset, uint256 amount) internal {
        tint256 storage delta = self.deltas[asset];
        delta.set(MixedSignLib.add(delta.get(), amount));
    }

    function sub(DeltaTracker storage self, address asset, uint256 amount)
        internal
        returns (int256 newDelta)
    {
        tint256 storage delta = self.deltas[asset];
        delta.set(newDelta = MixedSignLib.sub(delta.get(), amount));
    }
}
