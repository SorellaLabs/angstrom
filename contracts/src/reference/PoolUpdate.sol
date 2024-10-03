// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import {SafeCastLib} from "solady/src/utils/SafeCastLib.sol";
import {Pair, PairLib} from "./Pair.sol";

import {FormatLib} from "super-sol/libraries/FormatLib.sol";

struct PoolUpdate {
    address assetIn;
    address assetOut;
    uint128 amountIn;
    RewardsUpdate rewardUpdate;
}

struct RewardsUpdate {
    bool onlyCurrent;
    uint128 onlyCurrentQuantity;
    int24 startTick;
    uint128 startLiquidity;
    uint128[] quantities;
}

using PoolUpdateLib for PoolUpdate global;
using PoolUpdateLib for RewardsUpdate global;

/// @author philogy <https://github.com/philogy>
library PoolUpdateLib {
    using FormatLib for *;
    using SafeCastLib for uint256;
    using PairLib for Pair[];

    function encode(PoolUpdate[] memory updates, Pair[] memory pairs)
        internal
        pure
        returns (bytes memory b)
    {
        for (uint256 i = 0; i < updates.length; i++) {
            b = bytes.concat(b, updates[i].encode(pairs));
        }
        b = bytes.concat(bytes3(b.length.toUint24()), b);
    }

    function encode(PoolUpdate memory self, Pair[] memory pairs)
        internal
        pure
        returns (bytes memory)
    {
        (uint16 pairIndex, bool zeroForOne) = pairs.getIndex(self.assetIn, self.assetOut);
        uint8 variantByte = (zeroForOne ? 1 : 0) | (self.rewardUpdate.onlyCurrent ? 2 : 0);
        return bytes.concat(
            bytes1(variantByte),
            bytes2(pairIndex),
            bytes16(self.amountIn),
            self.rewardUpdate.encode()
        );
    }

    function encode(RewardsUpdate memory self) internal pure returns (bytes memory) {
        if (self.onlyCurrent) {
            return bytes.concat(bytes16(self.onlyCurrentQuantity));
        }
        bytes memory encodedQuantities;
        for (uint256 i = 0; i < self.quantities.length; i++) {
            encodedQuantities = bytes.concat(encodedQuantities, bytes16(self.quantities[i]));
        }
        encodedQuantities =
            bytes.concat(bytes3(encodedQuantities.length.toUint24()), encodedQuantities);

        return bytes.concat(
            bytes3(uint24(self.startTick)), bytes16(self.startLiquidity), encodedQuantities
        );
    }

    function total(RewardsUpdate memory self) internal pure returns (uint128 sum) {
        if (self.onlyCurrent) {
            sum = self.onlyCurrentQuantity;
        } else {
            for (uint256 i = 0; i < self.quantities.length; i++) {
                sum += self.quantities[i];
            }
        }
    }

    function toStr(RewardsUpdate memory self) internal pure returns (string memory) {
        if (self.onlyCurrent) {
            return string.concat(
                "RewardsUpdate::CurrentOnly { amount: ", self.onlyCurrentQuantity.toStr(), " }"
            );
        }
        string memory quantities = "";
        for (uint256 i = 0; i < self.quantities.length; i++) {
            quantities = string.concat(quantities, self.quantities[i].toStr(), ", ");
        }
        return string.concat(
            "RewardsUpdate::MultiTick { start_tick: ",
            self.startTick.toStr(),
            ", start_liquidity: ",
            self.startLiquidity.toStr(),
            ", quantities: [",
            quantities,
            "] }"
        );
    }
}
