// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import {SafeCastLib} from "solady/src/utils/SafeCastLib.sol";
import {Asset, AssetLib} from "./Asset.sol";

import {DEBUG_LOGS} from "../modules/DevFlags.sol";
import {console} from "forge-std/console.sol";

struct PoolUpdate {
    address assetIn;
    address assetOut;
    uint128 amountIn;
    RewardsUpdate rewardUpdate;
}

struct RewardsUpdate {
    bool below;
    int24 startTick;
    uint128 startLiquidity;
    uint128[] quantities;
}

using PoolUpdateLib for PoolUpdate global;
using PoolUpdateLib for RewardsUpdate global;

/// @author philogy <https://github.com/philogy>
library PoolUpdateLib {
    using SafeCastLib for uint256;
    using AssetLib for Asset[];

    function encode(PoolUpdate[] memory updates, Asset[] memory assets) internal pure returns (bytes memory b) {
        for (uint256 i = 0; i < updates.length; i++) {
            b = bytes.concat(b, updates[i].encode(assets));
        }
        b = bytes.concat(bytes3(b.length.toUint24()), b);
        if (DEBUG_LOGS) {
            console.log("PoolUpdate[] bytes: %x (%s)", b.length, b.length);
            console.logBytes(b);
        }
    }

    function encode(PoolUpdate memory self, Asset[] memory assets) internal pure returns (bytes memory) {
        (uint16 indexA, uint16 indexB) = assets.getIndexPair({assetA: self.assetIn, assetB: self.assetOut});
        return bytes.concat(bytes2(indexA), bytes2(indexB), bytes16(self.amountIn), self.rewardUpdate.encode());
    }

    function encode(RewardsUpdate memory self) internal pure returns (bytes memory) {
        bytes memory encodedQuantities;
        for (uint256 i = 0; i < self.quantities.length; i++) {
            encodedQuantities = bytes.concat(encodedQuantities, bytes16(self.quantities[i]));
        }
        encodedQuantities = bytes.concat(bytes3(encodedQuantities.length.toUint24()), encodedQuantities);

        return bytes.concat(
            self.below ? bytes1(0x01) : bytes1(0x00),
            bytes3(uint24(self.startTick)),
            bytes16(self.startLiquidity),
            encodedQuantities
        );
    }
}
