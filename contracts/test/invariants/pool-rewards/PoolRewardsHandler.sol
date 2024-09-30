// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "test/_helpers/BaseTest.sol";
import {PoolId} from "v4-core/src/types/PoolId.sol";
import {PRNG} from "super-sol/collections/PRNG.sol";
import {MockERC20} from "super-sol/mocks/MockERC20.sol";
import {ExtAngstrom} from "../../_view-ext/ExtAngstrom.sol";
import {UniV4Inspector} from "test/_view-ext/UniV4Inspector.sol";
import {PoolGate} from "test/_helpers/PoolGate.sol";
import {TickLib} from "src/libraries/TickLib.sol";
import {TickReward, RewardLib} from "test/_helpers/RewardLib.sol";
import {UsedIndexMap} from "super-sol/collections/UsedIndexMap.sol";
import {PoolId, PoolIdLibrary} from "v4-core/src/types/PoolId.sol";
import {TickMath} from "v4-core/src/libraries/TickMath.sol";
import {BalanceDelta} from "v4-core/src/types/BalanceDelta.sol";
import {ConversionLib} from "src/libraries/ConversionLib.sol";
import {PoolConfigStore} from "src/libraries/pool-config/PoolConfigStore.sol";

import {LibSort} from "solady/src/utils/LibSort.sol";

import {Bundle, Pair} from "src/reference/Bundle.sol";
import {Asset} from "src/reference/Asset.sol";
import {PoolUpdate, RewardsUpdate} from "src/reference/PoolUpdate.sol";
import {TopOfBlockOrder} from "src/reference/OrderTypes.sol";

import {EnumerableSetLib} from "solady/src/utils/EnumerableSetLib.sol";
import {FormatLib} from "super-sol/libraries/FormatLib.sol";
import {console} from "forge-std/console.sol";

int24 constant TICK_SPACING = 60;

/// @author philogy <https://github.com/philogy>
contract PoolRewardsHandler is BaseTest {
    using FormatLib for *;
    using EnumerableSetLib for EnumerableSetLib.Int256Set;
    using ConversionLib for *;
    using TickLib for int24;

    using RewardLib for TickReward[];

    UniV4Inspector public immutable uniV4;
    ExtAngstrom public immutable angstrom;
    PoolGate public immutable gate;
    PoolId public immutable id;
    PoolId public immutable refId;

    /// @dev Uniswap's `MIN_SQRT_RATIO + 1` to pass the limit check.
    uint160 internal constant MIN_SQRT_RATIO = 4295128740;
    /// @dev Uniswap's `MAX_SQRT_RATIO - 1` to pass the limit check.
    uint160 internal constant MAX_SQRT_RATIO = 1461446703485210103287273052203988822378723970341;

    address public immutable asset0;
    address public immutable asset1;
    MockERC20 public immutable token0;
    MockERC20 public immutable token1;
    Account public rewarder = makeAccount("rewarder");

    constructor(
        UniV4Inspector uniV4_,
        ExtAngstrom angstrom_,
        PoolGate gate_,
        PoolId id_,
        PoolId refId_,
        address asset0_,
        address asset1_,
        address gov
    ) {
        uniV4 = uniV4_;
        angstrom = angstrom_;
        gate = gate_;
        id = id_;
        refId = refId_;
        asset0 = asset0_;
        token0 = MockERC20(asset0_);
        asset1 = asset1_;
        token1 = MockERC20(asset1_);

        vm.prank(rewarder.addr);
        token0.approve(address(angstrom), type(uint256).max);
        vm.prank(rewarder.addr);
        token1.approve(address(angstrom), type(uint256).max);

        {
            address[] memory newNodes = new address[](1);
            newNodes[0] = rewarder.addr;
            vm.prank(gov);
            angstrom.toggleNodes(newNodes);
        }
    }

    struct Position {
        int24 lowerTick;
        int24 upperTick;
        uint256 liquidity;
    }

    EnumerableSetLib.Int256Set internal _ghost_initializedTicks;
    EnumerableSetLib.Int256Set internal _ghost_liquidInitializedTicks;
    int24 public ghost_lowestTick = type(int24).max;
    int24 public ghost_highestTick = type(int24).min;
    Position[] internal _ghost_postitions;

    mapping(int24 => uint256) public ghost_liquidityAtTick;

    function ghost_initializedTicks() public view returns (int24[] memory ticks) {
        int256[] memory initialized = _ghost_initializedTicks.values();
        assembly ("memory-safe") {
            ticks := initialized
        }
    }

    function ghost_positions() public view returns (Position[] memory) {
        return _ghost_postitions;
    }

    function addLiquidity(address sender, int24 lowerTick, int24 upperTick, uint256 liquidity)
        public
    {
        assertGt(liquidity, 0, "Liquidity zero");
        assertLt(lowerTick, upperTick, "Upper tick below or equal to lower tick");
        assertEq(lowerTick % TICK_SPACING, 0, "Lower tick incorrectly spaced");
        assertEq(upperTick % TICK_SPACING, 0, "Lower tick incorrectly spaced");
        vm.startPrank(sender);
        gate.setHook(address(angstrom));
        gate.addLiquidity(
            address(asset0), address(asset1), lowerTick, upperTick, liquidity, bytes32(0)
        );
        gate.setHook(address(0));
        gate.addLiquidity(
            address(asset0), address(asset1), lowerTick, upperTick, liquidity, bytes32(0)
        );
        vm.stopPrank();
        _initializeTick(lowerTick);
        _initializeTick(upperTick);
        _ghost_postitions.push(Position(lowerTick, upperTick, liquidity));

        for (int24 tick = lowerTick; tick < upperTick; tick += TICK_SPACING) {
            ghost_liquidityAtTick[tick] += liquidity;
            if (liquidity > 0 && _ghost_initializedTicks.contains(tick)) {
                _ghost_liquidInitializedTicks.add(tick);
            }
        }
    }

    function r() public view returns (address) {
        return rewarder.addr;
    }

    TickReward[] _ghost_tickRewards;

    function ghost_tickRewards() public view returns (TickReward[] memory) {
        return _ghost_tickRewards;
    }

    modifier passesTime() {
        vm.roll(block.number + 1);
        _;
    }

    function swapToPrice(uint160 targetSqrtPrice) public passesTime {
        int24 lowest = ghost_lowestTick;
        int24 highest = ghost_highestTick;
        assertGt(highest, lowest, "Less than 2 unique ticks initialized");
        targetSqrtPrice = uint160(
            bound(
                targetSqrtPrice,
                TickMath.getSqrtPriceAtTick(lowest),
                TickMath.getSqrtPriceAtTick(highest)
            )
        );

        _swapTo(targetSqrtPrice);
    }

    function swapToBoundary(uint256 tickIndex) public passesTime {
        tickIndex = bound(tickIndex, 0, _ghost_initializedTicks.length() - 1);
        int24 targetTick = int24(_ghost_initializedTicks.at(tickIndex));
        uint160 targetSqrtPrice = TickMath.getSqrtPriceAtTick(targetTick);
        _swapTo(targetSqrtPrice);
    }

    function _swapTo(uint160 targetSqrtPrice) internal {
        (uint160 currentPrice,,,,,,) = uniV4.getPool(id);

        if (targetSqrtPrice == currentPrice) return;

        bool zeroForOne = targetSqrtPrice < currentPrice;
        (MockERC20 assetIn, MockERC20 assetOut) = zeroForOne ? (token0, token1) : (token1, token0);

        gate.setHook(address(0));
        // Do initial swap to price to get delta.
        uint256 snapshot = vm.snapshot();
        BalanceDelta delta =
            gate.swap(address(assetIn), address(assetOut), type(int256).min, targetSqrtPrice);
        if (delta.amount0() == 0 && delta.amount1() == 0) {
            require(vm.revertTo(snapshot), "failed to revert");
            return;
        }
        // Swap back to original price.
        gate.swap(address(assetOut), address(assetIn), type(int256).min, currentPrice);
        delta = gate.swap(
            address(assetIn),
            address(assetOut),
            zeroForOne ? delta.amount0() : delta.amount1(),
            zeroForOne ? MIN_SQRT_RATIO : MAX_SQRT_RATIO
        );

        Bundle memory bundle = zeroForOne
            ? _newBundle(uint128(-delta.amount0()), 0)
            : _newBundle(0, uint128(-delta.amount1()));

        bundle.poolUpdates = new PoolUpdate[](1);
        PoolUpdate memory poolUpdate = bundle.poolUpdates[0];
        poolUpdate.assetIn = address(assetIn);
        poolUpdate.assetOut = address(assetOut);
        poolUpdate.amountIn = zeroForOne ? uint128(-delta.amount0()) : uint128(-delta.amount1());
        bundle.addDeltas(0, 1, delta);

        poolUpdate.rewardUpdate.onlyCurrent = true;

        PoolConfigStore store = angstrom.configStore();
        vm.prank(rewarder.addr);
        bytes memory encoded = bundle.encode(PoolConfigStore.unwrap(store));
        angstrom.execute(encoded);
    }

    function rewardLiquidity(uint256 ticksToReward, PRNG memory rng) public passesTime {
        uint256 totalTicks = _ghost_liquidInitializedTicks.length();

        ticksToReward = bound(ticksToReward, 0, totalTicks);

        // Select ticks & amounts to reward with.
        UsedIndexMap memory map;
        map.init(totalTicks, totalTicks / 4);
        TickReward[] memory rewards = new TickReward[](ticksToReward);
        for (uint256 i = 0; i < ticksToReward; i++) {
            int24 tick = int24(_ghost_liquidInitializedTicks.at(rng.useRandIndex(map)));
            uint128 amount = u128(rng.randchoice(0.1e18, 0, rng.randmag(0.01e18, 100.0e18)));
            rewards[i] = TickReward({tick: tick, amount: amount});
        }

        rewardTicks(rewards);
    }

    function rewardTicks(TickReward[] memory rewards) public {
        uint128 total = 0;
        for (uint256 i = 0; i < rewards.length; i++) {
            TickReward memory reward = rewards[i];
            total += reward.amount;
            _ghost_tickRewards.push(reward);
        }

        RewardsUpdate[] memory rewardUpdates = rewards.toUpdates(uniV4, id, TICK_SPACING);
        Bundle memory bundle = _newBundle(total, 0);
        bundle.poolUpdates = new PoolUpdate[](rewardUpdates.length);

        for (uint256 i = 0; i < rewardUpdates.length; i++) {
            PoolUpdate memory poolUpdate = bundle.poolUpdates[i];
            poolUpdate.assetIn = address(asset0);
            poolUpdate.assetOut = address(asset1);
            poolUpdate.rewardUpdate = rewardUpdates[i];
        }

        PoolConfigStore store = angstrom.configStore();
        vm.prank(rewarder.addr);
        bytes memory encoded = bundle.encode(PoolConfigStore.unwrap(store));
        angstrom.execute(encoded);
    }

    function ghost_totalInititalizedTicks() public view returns (uint256) {
        return _ghost_initializedTicks.length();
    }

    function _initializeTick(int24 tick) internal {
        _ghost_initializedTicks.add(tick);
        if (tick < ghost_lowestTick) ghost_lowestTick = tick;
        if (tick > ghost_highestTick) ghost_highestTick = tick;
    }

    function _newBundle(uint128 amount0, uint128 amount1) internal returns (Bundle memory bundle) {
        bundle.assets = new Asset[](2);
        bundle.assets[0].addr = asset0;
        bundle.assets[1].addr = asset1;
        bundle.pairs = new Pair[](1);
        bundle.pairs[0].asset0 = asset0;
        bundle.pairs[0].asset1 = asset1;

        uint256 length = (amount0 > 0 ? 1 : 0) + (amount1 > 0 ? 1 : 0);
        bundle.toBOrders = new TopOfBlockOrder[](length);
        if (amount0 > 0) {
            MockERC20(asset0).mint(rewarder.addr, amount0);
            TopOfBlockOrder memory tob = bundle.toBOrders[0];
            tob.quantityIn = amount0;
            _fillAndSign(tob, true);
        }
        if (amount1 > 0) {
            MockERC20(asset1).mint(rewarder.addr, amount1);
            TopOfBlockOrder memory tob = bundle.toBOrders[amount0 > 0 ? 1 : 0];
            tob.quantityIn = amount1;
            _fillAndSign(tob, false);
        }
    }

    function _fillAndSign(TopOfBlockOrder memory tob, bool zeroForOne) internal view {
        (tob.assetIn, tob.assetOut) =
            zeroForOne ? (address(asset0), address(asset1)) : (address(asset1), address(asset0));
        tob.validForBlock = u64(block.number);
        sign(rewarder, tob.meta, angstrom.hashTyped(tob.hash()));
    }
}
