// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {CalldataReader, CalldataReaderLib} from "src/types/CalldataReader.sol";
import {Asset, AssetArray, AssetLib} from "src/types/Asset.sol";
import {PairArray, PairLib} from "src/types/Pair.sol";
import {SwapCall, SwapCallLib} from "src/types/SwapCall.sol";
import {PoolUpdates} from "src/modules/PoolUpdates.sol";
import {Settlement} from "src/modules/Settlement.sol";
import {TopLevelAuth} from "src/modules/TopLevelAuth.sol";
import {UniConsumer} from "src/modules/UniConsumer.sol";
import {PoolId} from "v4-core/src/types/PoolId.sol";
import {POOL_FEE} from "src/Constants.sol";
import {IUniV4, IPoolManager} from "src/interfaces/IUniV4.sol";

import {console} from "forge-std/console.sol";
import {FormatLib} from "super-sol/libraries/FormatLib.sol";

/// @author philogy <https://github.com/philogy>
contract MockRewardsManager is UniConsumer, Settlement, PoolUpdates {
    using FormatLib for *;
    using IUniV4 for IPoolManager;

    constructor(IPoolManager uniV4, address controller)
        UniConsumer(uniV4)
        TopLevelAuth(controller)
        Settlement(address(0))
    {
        _checkAngstromHookFlags();
    }

    /// @param encoded PADE `(List<Asset>, List<Pair>, PoolUpdate)`.
    function update(bytes calldata encoded) public {
        CalldataReader reader = CalldataReaderLib.from(encoded);

        AssetArray assets;
        (reader, assets) = AssetLib.readFromAndValidate(reader);
        PairArray pairs;
        (reader, pairs) = PairLib.readFromAndValidate(reader, assets, _configStore);

        SwapCall memory swapCall = SwapCallLib.newSwapCall(address(this));
        reader = _updatePool(reader, swapCall, pairs);

        reader.requireAtEndOf(encoded);
    }

    function updateAfterTickMove(PoolId id, int24 lastTick, int24 newTick, int24 tickSpacing)
        external
    {
        poolRewards[id].updateAfterTickMove(id, UNI_V4, lastTick, newTick, tickSpacing);
    }

    function consts() external pure returns (uint24 poolFee) {
        poolFee = POOL_FEE;
    }

    function getGrowthInsideTick(PoolId id, int24 tick, int24 tickSpacing)
        public
        view
        returns (uint256)
    {
        _checkTickReal(id, tick, "tick");
        bool initialized;
        int24 nextTickUp;
        do {
            (initialized, nextTickUp) = UNI_V4.getNextTickGt(id, tick, tickSpacing);
        } while (!initialized);
        _checkTickReal(id, nextTickUp, "nextTickUp");
        return poolRewards[id].getGrowthInside(UNI_V4.getSlot0(id).tick(), tick, nextTickUp);
    }

    function getGrowthInsideRange(PoolId id, int24 lowerTick, int24 upperTick)
        public
        view
        returns (uint256)
    {
        _checkTickReal(id, lowerTick, "lowerTick");
        _checkTickReal(id, upperTick, "upperTick");
        return poolRewards[id].getGrowthInside(UNI_V4.getSlot0(id).tick(), lowerTick, upperTick);
    }

    function _checkTickReal(PoolId id, int24 tick, string memory name) internal view {
        (uint256 grossLiquidity,) = UNI_V4.getTickLiquidity(id, tick);
        require(grossLiquidity > 0, string.concat(name, " [", tick.toStr(), "] not initialized"));
    }
}
