// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {Pool} from "v4-core/src/libraries/Pool.sol";
import {BaseTest} from "test/_helpers/BaseTest.sol";
import {UniV4Inspector} from "test/_view-ext/UniV4Inspector.sol";
import {RouterActor} from "test/_mocks/RouterActor.sol";
import {OpenAngstrom} from "test/_mocks/OpenAngstrom.sol";
import {MockERC20} from "super-sol/mocks/MockERC20.sol";
import {EnumerableSetLib} from "solady/src/utils/EnumerableSetLib.sol";
import {PoolId, PoolIdLibrary} from "v4-core/src/types/PoolId.sol";
import {PoolKey} from "v4-core/src/types/PoolKey.sol";
import {Currency} from "v4-core/src/types/Currency.sol";
import {MAX_FEE, PoolConfigStore} from "src/libraries/PoolConfigStore.sol";
import {IUniV4, IPoolManager} from "src/interfaces/IUniV4.sol";
import {PRNG} from "super-sol/collections/PRNG.sol";
import {UsedIndexMap} from "super-sol/collections/UsedIndexMap.sol";
import {TickReward, RewardLib} from "test/_helpers/RewardLib.sol";
import {PoolUpdate, RewardsUpdate} from "test/_reference/PoolUpdate.sol";
import {Pair, PairLib} from "test/_reference/Pair.sol";
import {Asset, AssetLib} from "test/_reference/Asset.sol";
import {FixedPointMathLib} from "solady/src/utils/FixedPointMathLib.sol";

import {TickLib} from "src/libraries/TickLib.sol";
import {TickMath} from "v4-core/src/libraries/TickMath.sol";
import {SqrtPriceMath} from "v4-core/src/libraries/SqrtPriceMath.sol";
import {Slot0} from "v4-core/src/types/Slot0.sol";

import {console} from "forge-std/console.sol";
import {FormatLib} from "super-sol/libraries/FormatLib.sol";

struct Env {
    address owner;
    address controller;
    address feeMaster;
    UniV4Inspector uniV4;
    OpenAngstrom angstrom;
    MockERC20[] assets;
    MockERC20[] mirrors;
}

type AddKey is bytes32;

struct LiquidityAdd {
    int24 lowerTick;
    int24 upperTick;
    RouterActor owner;
    uint256 liquidity;
    // Reward stats.
    bool claimedRewards;
    uint256 rewardStartIndex;
    uint256 rewardEndIndex;
}

using LiquidityAddLib for LiquidityAdd global;

/// @author philogy <https://github.com/philogy>
contract AngstromHandler is BaseTest {
    using FormatLib for *;

    using PairLib for Pair[];
    using AssetLib for Asset[];
    using FixedPointMathLib for *;

    using PoolIdLibrary for PoolKey;
    using IUniV4 for UniV4Inspector;
    using TickLib for int24;
    using EnumerableSetLib for *;

    int24 constant MAX_TICK_WORDS_TRAVERSAL = 20;

    bytes32 internal constant DEFAULT_SALT = bytes32(0);

    Env e;

    EnumerableSetLib.AddressSet internal _routers;
    EnumerableSetLib.AddressSet internal _enabledAssets;

    // Sum of deposits by asset in angstrom.
    mapping(address asset => uint256 total) public ghost_totalDeposits;
    // Total rewards unclaimed by LPs.
    mapping(address asset => uint256 rewards) public ghost_pendingLpRewards;
    mapping(address asset => int256 saved) public ghost_netSavedDeltas;

    struct PoolInfo {
        uint256 asset0Index;
        uint256 asset1Index;
        int24 tickSpacing;
    }

    PoolInfo[] _ghost_createdPools;
    mapping(uint256 index0 => mapping(uint256 index1 => bool)) _ghost_poolInitialized;
    mapping(PoolId => int24[]) internal _ghost_initializedTicks;

    mapping(uint256 poolIndex => EnumerableSetLib.Uint256Set) internal _activeLiquidityIndices;
    mapping(uint256 poolIndex => LiquidityAdd[]) _liquidityAdds;
    mapping(uint256 poolIndex => TickReward[]) _tickRewards;

    constructor(Env memory env) {
        e = env;
    }

    // Router actor.
    address ra;
    RouterActor router;

    modifier routerWithNew(uint256 routerIndex) {
        unchecked {
            uint256 len = _routers.length();
            routerIndex = bound(routerIndex, 0, len);
            if (routerIndex < len) {
                ra = _routers.at(routerIndex);
                router = RouterActor(ra);
            } else {
                ra = address(router = new RouterActor(e.uniV4));
                vm.label(ra, string.concat("actor_", vm.toString(routerIndex + 1)));
                _routers.add(ra);
            }
        }

        _;
    }

    function initializePool(
        uint256 asset0Index,
        uint256 asset1Index,
        int24 tickSpacing,
        uint24 feeInE6,
        uint160 startSqrtPriceX96
    ) public {
        asset0Index = bound(asset0Index, 0, e.assets.length - 1);
        asset1Index = bound(asset1Index, 0, e.assets.length - 1);
        feeInE6 = uint24(bound(feeInE6, 0, MAX_FEE));
        startSqrtPriceX96 =
            uint160(bound(startSqrtPriceX96, TickMath.MIN_SQRT_PRICE, TickMath.MAX_SQRT_PRICE));
        if (asset0Index == asset1Index) {
            unchecked {
                asset1Index = (asset0Index + 1) % e.assets.length;
            }
        }
        if (asset0Index > asset1Index) (asset0Index, asset1Index) = (asset1Index, asset0Index);
        tickSpacing =
            int24(bound(tickSpacing, TickMath.MIN_TICK_SPACING, TickMath.MAX_TICK_SPACING));

        assertFalse(_ghost_poolInitialized[asset0Index][asset1Index]);
        _ghost_poolInitialized[asset0Index][asset1Index] = true;
        uint256 storeIndex = e.angstrom.configStore().totalEntries();

        address asset0 = address(e.assets[asset0Index]);
        address mirror0 = address(e.mirrors[asset0Index]);
        address asset1 = address(e.assets[asset1Index]);
        address mirror1 = address(e.mirrors[asset1Index]);

        vm.prank(e.controller);
        e.angstrom.configurePool(asset0, asset1, uint16(uint24(tickSpacing)), feeInE6);

        _enabledAssets.add(asset0);
        _enabledAssets.add(asset1);

        e.angstrom.initializePool(asset0, asset1, storeIndex, startSqrtPriceX96);

        e.uniV4.initialize(poolKey(mirror0, mirror1, tickSpacing), startSqrtPriceX96);

        _ghost_createdPools.push(PoolInfo(asset0Index, asset1Index, tickSpacing));
    }

    function addLiquidity(
        uint256 poolIndex,
        uint256 routerIndex,
        int24 lowerTick,
        int24 upperTick,
        uint256 liquidity
    ) public routerWithNew(routerIndex) {
        if (DEBUG) console.log("\n[BIG ADD BLOCK]");
        poolIndex = bound(poolIndex, 0, _ghost_createdPools.length - 1);
        PoolInfo storage pool = _ghost_createdPools[poolIndex];
        {
            (int24 minTick, int24 maxTick) = _getBounds(pool.tickSpacing);
            lowerTick =
                int24(bound(lowerTick, minTick, maxTick)).normalizeUnchecked(pool.tickSpacing);
            upperTick =
                int24(bound(upperTick, minTick, maxTick)).normalizeUnchecked(pool.tickSpacing);
            vm.assume(lowerTick != upperTick);
            if (upperTick < lowerTick) (lowerTick, upperTick) = (upperTick, lowerTick);
        }

        {
            PoolKey memory actualKey = poolKey(
                e.angstrom,
                address(e.assets[pool.asset0Index]),
                address(e.assets[pool.asset1Index]),
                pool.tickSpacing
            );
            PoolId id = actualKey.toId();

            (uint128 maxNetLiquidity, uint256 amount0, uint256 amount1) =
                getMaxNetLiquidity(id, lowerTick, upperTick, pool);
            vm.assume(maxNetLiquidity > 0);
            liquidity = bound(liquidity, 1, maxNetLiquidity);

            if (!e.uniV4.isInitialized(id, lowerTick, pool.tickSpacing)) {
                _addToTickList(_ghost_initializedTicks[id], lowerTick);
            }

            if (!e.uniV4.isInitialized(id, upperTick, pool.tickSpacing)) {
                _addToTickList(_ghost_initializedTicks[id], upperTick);
            }

            {
                MockERC20 mirror0 = e.mirrors[pool.asset0Index];
                MockERC20 mirror1 = e.mirrors[pool.asset1Index];
                mirror0.transfer(ra, amount0);
                mirror1.transfer(ra, amount1);

                router.modifyLiquidity(
                    poolKey(address(mirror0), address(mirror1), pool.tickSpacing),
                    lowerTick,
                    upperTick,
                    int256(liquidity),
                    DEFAULT_SALT
                );
            }

            {
                MockERC20 asset0 = e.assets[pool.asset0Index];
                MockERC20 asset1 = e.assets[pool.asset1Index];
                asset0.transfer(ra, amount0);
                asset1.transfer(ra, amount1);
                router.modifyLiquidity(
                    actualKey, lowerTick, upperTick, int256(liquidity), DEFAULT_SALT
                );
            }
        }

        uint256 newIndex = _liquidityAdds[poolIndex].length;
        if (DEBUG) {
            console.log("[add]");
            console.log("  newIndex: %s", newIndex);
            console.log("  lowerTick: %s", lowerTick.toStr());
            console.log("  upperTick: %s", upperTick.toStr());
            console.log("  liquidity: %s", liquidity);
        }
        _activeLiquidityIndices[poolIndex].add(newIndex);
        _liquidityAdds[poolIndex].push(
            LiquidityAdd(
                lowerTick,
                upperTick,
                router,
                liquidity,
                false,
                _tickRewards[poolIndex].length,
                type(uint256).max
            )
        );
    }

    function removeLiquidity(
        uint256 poolIndex,
        uint256 liquidityRelativeIndex,
        uint256 liquidityToRemove
    ) public {
        if (DEBUG) console.log("\n[BIG REMOVE BLOCK]");
        poolIndex = bound(poolIndex, 0, _ghost_createdPools.length - 1);
        uint256 totalActive = _activeLiquidityIndices[poolIndex].length();
        vm.assume(totalActive > 0);
        uint256 index =
            _activeLiquidityIndices[poolIndex].at(bound(liquidityRelativeIndex, 0, totalActive - 1));
        _activeLiquidityIndices[poolIndex].remove(index);
        LiquidityAdd storage liqAdd = _liquidityAdds[poolIndex][index];
        liquidityToRemove = bound(liquidityToRemove, 0, liqAdd.liquidity);

        if (DEBUG) {
            console.log("[remove]");
            console.log("  lowerTick: %s", liqAdd.lowerTick.toStr());
            console.log("  upperTick: %s", liqAdd.upperTick.toStr());
            console.log("  liquidityToRemove: %s", liquidityToRemove);
        }

        {
            PoolInfo storage pool = _ghost_createdPools[poolIndex];
            PoolKey memory actualKey = poolKey(
                e.angstrom,
                address(e.assets[pool.asset0Index]),
                address(e.assets[pool.asset1Index]),
                pool.tickSpacing
            );
            ghost_pendingLpRewards[address(e.assets[pool.asset0Index])] -= e
                .angstrom
                .getPositionRewards(
                actualKey.toId(),
                address(liqAdd.owner),
                liqAdd.lowerTick,
                liqAdd.upperTick,
                DEFAULT_SALT
            );

            liqAdd.owner.modifyLiquidity(
                actualKey,
                liqAdd.lowerTick,
                liqAdd.upperTick,
                -int256(liquidityToRemove),
                DEFAULT_SALT
            );
            liqAdd.owner.recycle(address(e.assets[pool.asset0Index]));
            liqAdd.owner.recycle(address(e.assets[pool.asset1Index]));
            liqAdd.rewardEndIndex = _tickRewards[poolIndex].length;

            {
                MockERC20 mirror0 = e.mirrors[pool.asset0Index];
                MockERC20 mirror1 = e.mirrors[pool.asset1Index];
                liqAdd.owner.modifyLiquidity(
                    poolKey(address(mirror0), address(mirror1), pool.tickSpacing),
                    liqAdd.lowerTick,
                    liqAdd.upperTick,
                    -int256(liquidityToRemove),
                    DEFAULT_SALT
                );
                liqAdd.owner.recycle(address(mirror0));
                liqAdd.owner.recycle(address(mirror1));
            }

            PoolId id = actualKey.toId();

            if (!e.uniV4.isInitialized(id, liqAdd.lowerTick, pool.tickSpacing)) {
                _removeTick(_ghost_initializedTicks[id], liqAdd.lowerTick);
            }

            if (!e.uniV4.isInitialized(id, liqAdd.upperTick, pool.tickSpacing)) {
                _removeTick(_ghost_initializedTicks[id], liqAdd.upperTick);
            }
        }

        uint256 totalLiquidity = liqAdd.liquidity;
        {
            LiquidityAdd[] storage adds = _liquidityAdds[poolIndex];
            uint256[] memory relIndices = _activeLiquidityIndices[poolIndex].values();
            for (uint256 ri = 0; ri < relIndices.length; ri++) {
                uint256 i = relIndices[ri];
                LiquidityAdd storage ca = adds[i];
                if (
                    !ca.claimedRewards && ca.owner == liqAdd.owner
                        && ca.lowerTick == liqAdd.lowerTick && ca.upperTick == ca.upperTick
                ) {
                    ca.claimedRewards = true;
                    totalLiquidity += ca.liquidity;
                    _activeLiquidityIndices[poolIndex].remove(i);
                }
            }
        }

        if (liquidityToRemove < totalLiquidity) {
            uint256 newIndex = _liquidityAdds[poolIndex].length;
            _activeLiquidityIndices[poolIndex].add(newIndex);
            if (DEBUG) {
                console.log("  [partial]");
                console.log("  newIndex: %s", newIndex);
                console.log("  newLiquidity: %s", totalLiquidity - liquidityToRemove);
            }
            _liquidityAdds[poolIndex].push(
                LiquidityAdd(
                    liqAdd.lowerTick,
                    liqAdd.upperTick,
                    liqAdd.owner,
                    totalLiquidity - liquidityToRemove,
                    false,
                    _tickRewards[poolIndex].length,
                    type(uint256).max
                )
            );
        }
    }

    function rewardTicks(uint256 poolIndex, uint256 ticksToReward, PRNG memory rng) public {
        if (DEBUG) console.log("\n[BIG REWARD BLOCK]");
        poolIndex = bound(poolIndex, 0, _ghost_createdPools.length - 1);
        PoolInfo storage pool = _ghost_createdPools[poolIndex];
        PoolId id = poolKey(
            e.angstrom,
            address(e.assets[pool.asset0Index]),
            address(e.assets[pool.asset1Index]),
            pool.tickSpacing
        ).toId();

        int24[] memory rewardableTicks = _getRewardableTicks(id, pool.tickSpacing);
        uint256 totalTicks = rewardableTicks.length;
        ticksToReward = bound(ticksToReward, 0, totalTicks);

        UsedIndexMap memory map;
        map.init(totalTicks, totalTicks / 4);
        TickReward[] memory rewards = new TickReward[](ticksToReward);
        uint256 total = 0;
        for (uint256 i = 0; i < ticksToReward; i++) {
            int24 tick = int24(rewardableTicks[rng.useRandIndex(map)]);
            uint128 amount =
                u128(rng.randuint(1.0e18) <= 0.1e18 ? 0 : rng.randmag(1, type(uint104).max));
            rewards[i] = TickReward({tick: tick, amount: amount});
            total += amount;
            if (DEBUG) {
                console.log("rewardTicks:");
                console.log("  tick: %s", tick.toStr());
                console.log("  amount: %s", amount);
            }
            _tickRewards[poolIndex].push(rewards[i]);
        }

        RewardsUpdate[] memory rewardUpdates =
            RewardLib.toUpdates(rewards, e.uniV4, id, pool.tickSpacing);

        address asset0 = address(e.assets[pool.asset0Index]);
        address asset1 = address(e.assets[pool.asset1Index]);

        ghost_pendingLpRewards[asset0] += total;
        MockERC20 rewardAsset = e.assets[pool.asset0Index];
        rewardAsset.mint(address(this), total);
        rewardAsset.approve(address(e.angstrom), type(uint256).max);
        e.angstrom.deposit(address(asset0), total);

        uint256 totalUpdates = rewardUpdates.length;
        if (totalUpdates > 0) {
            Asset[] memory assets = new Asset[](2);
            assets[0].addr = asset0;
            assets[1].addr = asset1;
            Pair[] memory pairs = new Pair[](1);
            pairs[0].asset0 = asset0;
            pairs[0].asset1 = asset1;
            if (DEBUG) {
                int24 currentTick = e.uniV4.getSlot0(id).tick();
                console.log("[updates] (%s)", currentTick.toStr());
            }
            for (uint256 i = 0; i < totalUpdates; i++) {
                if (DEBUG) {
                    console.log("%s:", i);
                    RewardsUpdate memory r = rewardUpdates[i];
                    if (r.onlyCurrent) {
                        console.log("  OnlyCurrent(%s)", r.onlyCurrentQuantity);
                    } else {
                        console.log("  MultiTick:");
                        console.log("    startTick: %s", r.startTick.toStr());
                        console.log("    startLiquidity: %s", r.startLiquidity);
                        console.log("    quantities: %s", r.quantities.toStr());
                    }
                }
                PoolUpdate memory update = PoolUpdate(asset0, asset1, 0, rewardUpdates[i]);
                e.angstrom.updatePool(
                    bytes.concat(
                        assets.encode(),
                        pairs.encode(assets, PoolConfigStore.unwrap(e.angstrom.configStore())),
                        update.encode(pairs)
                    )
                );
            }
        }

        _saveDeltas();
    }

    function enabledAssets() public view returns (address[] memory) {
        return _enabledAssets.values();
    }

    function actors() public view returns (address[] memory) {
        return _routers.values();
    }

    function routers() public view returns (address[] memory) {
        return _routers.values();
    }

    function liquidityAdds(uint256 poolIndex) public view returns (LiquidityAdd[] memory) {
        return _liquidityAdds[poolIndex];
    }

    function tickRewards(uint256 poolIndex) public view returns (TickReward[] memory) {
        return _tickRewards[poolIndex];
    }

    function poolIndexToId(uint256 poolIndex) public view returns (PoolId) {
        PoolInfo storage pool = _ghost_createdPools[poolIndex];
        return poolKey(
            e.angstrom,
            address(e.assets[pool.asset0Index]),
            address(e.assets[pool.asset1Index]),
            pool.tickSpacing
        ).toId();
    }

    function totalPools() public view returns (uint256) {
        return _ghost_createdPools.length;
    }

    function getPool(uint256 poolIndex)
        public
        view
        returns (address asset0, address asset1, int24 tickSpacing)
    {
        PoolInfo storage pool = _ghost_createdPools[poolIndex];
        asset0 = address(e.assets[pool.asset0Index]);
        asset1 = address(e.assets[pool.asset1Index]);
        tickSpacing = pool.tickSpacing;
    }

    function _removeTick(int24[] storage ticks, int24 tick) internal {
        uint256 len = ticks.length;
        uint256 i = 0;
        for (; i < len; i++) {
            if (ticks[i] == tick) break;
        }
        for (; i < len - 1; i++) {
            ticks[i] = ticks[i + 1];
        }
        ticks.pop();
    }

    function _addToTickList(int24[] storage ticks, int24 tick) internal {
        uint256 len = ticks.length;
        uint256 i = 0;

        for (; i < len; i++) {
            if (tick < ticks[i]) break;
        }
        for (; i < len; i++) {
            (ticks[i], tick) = (tick, ticks[i]);
        }

        ticks.push(tick);
    }

    function _getBounds(int24 tickSpacing) internal pure returns (int24 minTick, int24 maxTick) {
        minTick = TickMath.MIN_TICK / tickSpacing * tickSpacing;
        maxTick = TickMath.MAX_TICK / tickSpacing * tickSpacing;
    }

    function _getRewardableTicks(PoolId id, int24 tickSpacing)
        internal
        view
        returns (int24[] memory ticks)
    {
        int24 current = e.uniV4.getSlot0(id).tick().normalizeUnchecked(tickSpacing);
        int24 distance = MAX_TICK_WORDS_TRAVERSAL * 256 * tickSpacing;
        int24 lowest = current - distance;
        int24 highest = current + distance;

        uint256 i = 0;
        int24[] storage poolTicks = _ghost_initializedTicks[id];
        uint256 len = poolTicks.length;
        for (; i < len; i++) {
            if (poolTicks[i] >= lowest) break;
        }
        uint256 startIndex = i;
        ticks = new int24[](len - startIndex);
        for (; i < len; i++) {
            if (poolTicks[i] > highest) break;
            unchecked {
                ticks[i - startIndex] = poolTicks[i];
            }
        }
        assembly ("memory-safe") {
            mstore(ticks, sub(i, startIndex))
        }
    }

    function _saveDeltas() internal {
        uint256 totalAssets = _enabledAssets.length();
        for (uint256 i = 0; i < totalAssets; i++) {
            address asset = _enabledAssets.at(i);
            ghost_netSavedDeltas[asset] += e.angstrom.getDelta(asset);
        }
    }

    function getMaxNetLiquidity(PoolId id, int24 lowerTick, int24 upperTick, PoolInfo storage pool)
        internal
        view
        returns (uint128 maxNetLiquidity, uint256 amount0, uint256 amount1)
    {
        {
            uint128 maxLiquidityPerTick = Pool.tickSpacingToMaxLiquidityPerTick(pool.tickSpacing);
            (uint128 liquidityGrossLower,) = e.uniV4.getTickLiquidity(id, lowerTick);
            (uint128 liquidityGrossUpper,) = e.uniV4.getTickLiquidity(id, upperTick);
            maxNetLiquidity =
                maxLiquidityPerTick - uint128(max(liquidityGrossLower, liquidityGrossUpper));
        }

        Slot0 slot0 = e.uniV4.getSlot0(id);
        (int24 tick, uint160 sqrtPriceX96) = (slot0.tick(), slot0.sqrtPriceX96());

        (amount0, amount1) =
            getAddLiquidityDelta(tick, sqrtPriceX96, lowerTick, upperTick, maxNetLiquidity);

        uint256 maxAmount0 = e.assets[pool.asset0Index].balanceOf(address(this));
        uint256 maxAmount1 = e.assets[pool.asset1Index].balanceOf(address(this));

        maxNetLiquidity = u128(
            min(
                min(
                    maxAmount0 < amount0
                        ? maxNetLiquidity.fullMulDiv(maxAmount0, amount0)
                        : maxNetLiquidity,
                    maxAmount1 < amount1
                        ? maxNetLiquidity.fullMulDiv(maxAmount1, amount1)
                        : maxNetLiquidity
                ),
                maxNetLiquidity
            )
        );

        (amount0, amount1) =
            getAddLiquidityDelta(tick, sqrtPriceX96, lowerTick, upperTick, maxNetLiquidity);
    }

    function getAddLiquidityDelta(
        int24 tick,
        uint160 sqrtPriceX96,
        int24 lowerTick,
        int24 upperTick,
        uint128 liquidity
    ) internal pure returns (uint256 amount0, uint256 amount1) {
        if (tick < lowerTick) {
            amount0 = SqrtPriceMath.getAmount0Delta(
                TickMath.getSqrtPriceAtTick(lowerTick),
                TickMath.getSqrtPriceAtTick(upperTick),
                liquidity,
                true
            );
        } else if (tick < upperTick) {
            amount0 = SqrtPriceMath.getAmount0Delta(
                sqrtPriceX96, TickMath.getSqrtPriceAtTick(upperTick), liquidity, true
            );
            amount1 = SqrtPriceMath.getAmount1Delta(
                TickMath.getSqrtPriceAtTick(lowerTick), sqrtPriceX96, liquidity, true
            );
        } else {
            amount1 = SqrtPriceMath.getAmount1Delta(
                TickMath.getSqrtPriceAtTick(lowerTick),
                TickMath.getSqrtPriceAtTick(upperTick),
                liquidity,
                true
            );
        }
    }
}

library LiquidityAddLib {
    function key(LiquidityAdd memory self) internal pure returns (AddKey) {
        return AddKey.wrap(keccak256(abi.encode(self.lowerTick, self.upperTick, self.owner)));
    }
}
