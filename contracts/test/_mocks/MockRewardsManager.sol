// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {CalldataReader, CalldataReaderLib} from "../../src/types/CalldataReader.sol";
import {Asset, AssetArray, AssetLib} from "../../src/types/Asset.sol";
import {PoolUpdateManager} from "../../src/modules/PoolUpdateManager.sol";
import {SettlementManager} from "../../src/modules/SettlementManager.sol";
import {UniConsumer} from "../../src/modules/UniConsumer.sol";
import {PoolId} from "v4-core/src/types/PoolId.sol";
import {TICK_SPACING, POOL_FEE} from "../../src/Constants.sol";
import {IUniV4, IPoolManager} from "../../src/interfaces/IUniV4.sol";

import {console} from "forge-std/console.sol";

/// @author philogy <https://github.com/philogy>
contract MockRewardsManager is UniConsumer, SettlementManager, PoolUpdateManager {
    using IUniV4 for IPoolManager;

    constructor(address uniV4PoolManager) UniConsumer(uniV4PoolManager) {
        console.log("rewards manager deployed");
    }

    /// @param encoded PADE `(List<Asset>, PoolUpdate)`.
    function update(bytes calldata encoded) public {
        CalldataReader reader = CalldataReaderLib.from(encoded);

        AssetArray assets;
        (reader, assets) = AssetLib.readFromAndValidate(reader);

        reader = _updatePool(reader, tBundleDeltas, assets);

        reader.requireAtEndOf(encoded);
    }

    function updateAfterTickMove(PoolId id, int24 lastTick, int24 newTick) external {
        poolRewards[id].updateAfterTickMove(id, UNI_V4, lastTick, newTick);
    }

    function consts() external pure returns (int24 tickSpacing, uint24 poolFee) {
        tickSpacing = TICK_SPACING;
        poolFee = POOL_FEE;
    }

    function getGrowthInsideTick(PoolId id, int24 tick) public view returns (uint256) {
        _checkTickReal(id, tick, "tick");
        bool initialized;
        int24 nextTickUp;
        do {
            (initialized, nextTickUp) = _findNextTickUp(id, tick);
        } while (!initialized);
        _checkTickReal(id, nextTickUp, "nextTickUp");
        return poolRewards[id].getGrowthInside(_getCurrentTick(id), tick, nextTickUp);
    }

    function getGrowthInsideRange(PoolId id, int24 lowerTick, int24 upperTick) public view returns (uint256) {
        _checkTickReal(id, lowerTick, "lowerTick");
        _checkTickReal(id, upperTick, "upperTick");
        return poolRewards[id].getGrowthInside(_getCurrentTick(id), lowerTick, upperTick);
    }

    function _checkTickReal(PoolId id, int24 tick, string memory name) internal view {
        require(tick % TICK_SPACING == 0, string.concat(name, " not from regular spacing"));
        (uint256 grossLiquidity,) = UNI_V4.getTickLiquidity(id, tick);
        require(grossLiquidity > 0, string.concat(name, " not initialized"));
    }
}
