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

struct LiquidityAdd {
    uint256 liquidity;
    uint256 rewardStartIndex;
    uint256 rewardEndIndex;
}

struct LiquidityPosition {
    int24 lowerTick;
    int24 upperTick;
    RouterActor owner;
    uint256 totalRewardsX128;
    uint256 claimedRewards;
    uint256 totalLiquidity;
    uint256 activeAddsOffset;
    LiquidityAdd[] adds;
}

type PositionKey is bytes32;

function getKey(int24 lowerTick, int24 upperTick, RouterActor owner) pure returns (PositionKey) {
    return PositionKey.wrap(keccak256(abi.encode(lowerTick, upperTick, owner)));
}

using LiquidityLib for LiquidityPosition global;

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
    mapping(address asset => uint256 rewards) public ghost_totalLpRewards;
    mapping(address asset => uint256 rewards) public ghost_claimedLpRewards;
    mapping(address asset => uint256 rewards) public ghost_unclaimableRewards;
    mapping(address asset => int256 saved) public ghost_netSavedDeltas;

    struct PoolInfo {
        uint256 asset0Index;
        uint256 asset1Index;
        int24 tickSpacing;
    }

    PoolInfo[] _ghost_createdPools;
    mapping(uint256 index0 => mapping(uint256 index1 => bool)) _ghost_poolInitialized;
    mapping(PoolId => int24[]) internal _ghost_initializedTicks;

    mapping(uint256 poolIndex => TickReward[]) _tickRewards;
    mapping(uint256 poolIndex => mapping(PositionKey => LiquidityPosition)) _positions;
    mapping(uint256 poolIndex => EnumerableSetLib.Bytes32Set) _positionKeys;
    mapping(uint256 poolIndex => EnumerableSetLib.Bytes32Set) _activeKeys;

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

        PositionKey key = getKey(lowerTick, upperTick, router);

        if (DEBUG) {
            console.log("[add]");
            console.log("  lowerTick: %s", lowerTick.toStr());
            console.log("  upperTick: %s", upperTick.toStr());
            console.log("  liquidity: %s", liquidity);
            console.log("  owner: %s", ra);
        }
        _activeKeys[poolIndex].add(PositionKey.unwrap(key));
        LiquidityPosition storage position = _positions[poolIndex][key];
        if (_positionKeys[poolIndex].add(PositionKey.unwrap(key))) {
            position.lowerTick = lowerTick;
            position.upperTick = upperTick;
            position.owner = router;
        }
        position.totalLiquidity += liquidity;
        position.adds.push(
            LiquidityAdd(liquidity, _tickRewards[poolIndex].length, type(uint256).max)
        );
    }

    function removeLiquidity(
        uint256 poolIndex,
        uint256 liquidityRelativeIndex,
        uint256 liquidityToRemove
    ) public {
        if (DEBUG) console.log("\n[BIG REMOVE BLOCK]");
        poolIndex = bound(poolIndex, 0, _ghost_createdPools.length - 1);
        uint256 totalActive = _activeKeys[poolIndex].length();
        vm.assume(totalActive > 0);
        PositionKey key = PositionKey.wrap(
            _activeKeys[poolIndex].at(bound(liquidityRelativeIndex, 0, totalActive - 1))
        );

        LiquidityPosition storage position = _positions[poolIndex][key];
        liquidityToRemove = bound(liquidityToRemove, 0, position.totalLiquidity);

        if (DEBUG) {
            console.log("[remove]");
            console.log("  lowerTick: %s", position.lowerTick.toStr());
            console.log("  upperTick: %s", position.upperTick.toStr());
            console.log("  liquidityToRemove: %s", liquidityToRemove);
            console.log("  owner: %s", address(position.owner));
        }

        {
            PoolInfo storage pool = _ghost_createdPools[poolIndex];
            PoolKey memory actualKey = poolKey(
                e.angstrom,
                address(e.assets[pool.asset0Index]),
                address(e.assets[pool.asset1Index]),
                pool.tickSpacing
            );
            uint256 newRewards = e.angstrom.getPositionRewards(
                actualKey.toId(),
                address(position.owner),
                position.lowerTick,
                position.upperTick,
                DEFAULT_SALT
            );

            ghost_claimedLpRewards[address(e.assets[pool.asset0Index])] += newRewards;
            position.claimedRewards += newRewards;
            position.owner.modifyLiquidity(
                actualKey,
                position.lowerTick,
                position.upperTick,
                -int256(liquidityToRemove),
                DEFAULT_SALT
            );
            position.owner.recycle(address(e.assets[pool.asset0Index]));
            position.owner.recycle(address(e.assets[pool.asset1Index]));

            {
                MockERC20 mirror0 = e.mirrors[pool.asset0Index];
                MockERC20 mirror1 = e.mirrors[pool.asset1Index];
                position.owner.modifyLiquidity(
                    poolKey(address(mirror0), address(mirror1), pool.tickSpacing),
                    position.lowerTick,
                    position.upperTick,
                    -int256(liquidityToRemove),
                    DEFAULT_SALT
                );
                position.owner.recycle(address(mirror0));
                position.owner.recycle(address(mirror1));
            }

            PoolId id = actualKey.toId();

            if (!e.uniV4.isInitialized(id, position.lowerTick, pool.tickSpacing)) {
                _removeTick(_ghost_initializedTicks[id], position.lowerTick);
            }

            if (!e.uniV4.isInitialized(id, position.upperTick, pool.tickSpacing)) {
                _removeTick(_ghost_initializedTicks[id], position.upperTick);
            }
        }

        position.totalLiquidity -= liquidityToRemove;

        // delete all adds

        LiquidityAdd[] storage adds = position.adds;
        uint256 rewardsOffset = _tickRewards[poolIndex].length;

        for (uint256 i = position.activeAddsOffset; i < adds.length; i++) {
            adds[i].rewardEndIndex = rewardsOffset;
        }
        position.activeAddsOffset = adds.length;

        if (position.totalLiquidity > 0) {
            position.adds.push(
                LiquidityAdd(position.totalLiquidity, rewardsOffset, type(uint256).max)
            );
        } else {
            _activeKeys[poolIndex].remove(PositionKey.unwrap(key));
        }
    }

    PositionKey[] keysToReward;
    uint256 savedPoolIndex;

    function rewardTicks(uint256 poolIndex, uint256 ticksToReward, PRNG memory rng) public {
        if (DEBUG) console.log("\n[BIG REWARD BLOCK]");
        savedPoolIndex = poolIndex = bound(poolIndex, 0, _ghost_createdPools.length - 1);
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

            assembly ("memory-safe") {
                sstore(keysToReward.slot, 0)
            }
            uint256 totalKeys = _activeKeys[savedPoolIndex].length();
            uint256 liquidityClaimingReward = 0;
            for (uint256 j = 0; j < totalKeys; j++) {
                PositionKey key = PositionKey.wrap(_activeKeys[savedPoolIndex].at(j));
                LiquidityPosition storage pos = _positions[savedPoolIndex][key];
                if (pos.lowerTick <= tick && tick < pos.upperTick) {
                    liquidityClaimingReward += pos.totalLiquidity;
                    keysToReward.push(key);
                }
            }

            if (liquidityClaimingReward == 0) {
                ghost_unclaimableRewards[address(e.assets[pool.asset0Index])] += amount;
            }

            totalKeys = keysToReward.length;
            for (uint256 j = 0; j < totalKeys; j++) {
                PositionKey key = keysToReward[j];
                LiquidityPosition storage pos = _positions[savedPoolIndex][key];
                pos.totalRewardsX128 += (uint256(amount) * (1 << 128)).fullMulDiv(
                    pos.totalLiquidity, liquidityClaimingReward
                );
            }
        }

        RewardsUpdate[] memory rewardUpdates =
            RewardLib.toUpdates(rewards, e.uniV4, id, pool.tickSpacing);

        address asset0 = address(e.assets[pool.asset0Index]);
        address asset1 = address(e.assets[pool.asset1Index]);

        ghost_totalLpRewards[asset0] += total;
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
                        console.log("  MultiTick {");
                        console.log("    startTick: %s,", r.startTick.toStr());
                        console.log("    startLiquidity: %s,", r.startLiquidity);
                        console.log("    quantities: %s", r.quantities.toStr());
                        console.log("  }");
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

    function getPosition(uint256 poolIndex, PositionKey key)
        public
        view
        returns (LiquidityPosition memory)
    {
        return _positions[poolIndex][key];
    }

    function positionKeys(uint256 poolIndex) public view returns (PositionKey[] memory keys) {
        bytes32[] memory rawKeys = _positionKeys[poolIndex].values();
        assembly ("memory-safe") {
            keys := rawKeys
        }
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

library LiquidityLib {
    function key(LiquidityPosition memory self) internal pure returns (PositionKey) {
        return PositionKey.wrap(keccak256(abi.encode(self.lowerTick, self.upperTick, self.owner)));
    }
}
