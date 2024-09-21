// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "test/_helpers/BaseTest.sol";
import {PoolId} from "v4-core/src/types/PoolId.sol";
import {PRNG} from "super-sol/collections/PRNG.sol";
import {MockERC20} from "super-sol/mocks/MockERC20.sol";
import {TICK_SPACING} from "src/Constants.sol";
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

import {LibSort} from "solady/src/utils/LibSort.sol";

import {Bundle} from "src/reference/Bundle.sol";
import {Asset} from "src/reference/Asset.sol";
import {PoolUpdate, RewardsUpdate} from "src/reference/PoolUpdate.sol";
import {TopOfBlockOrder} from "src/reference/OrderTypes.sol";

import {EnumerableSetLib} from "solady/src/utils/EnumerableSetLib.sol";
import {console} from "forge-std/console.sol";
import {FormatLib} from "super-sol/libraries/FormatLib.sol";
import {TEST_LOGS} from "src/modules/DevFlags.sol";

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

    MockERC20 public immutable asset0;
    MockERC20 public immutable asset1;
    Account public rewarder = makeAccount("rewarder");

    constructor(
        UniV4Inspector uniV4_,
        ExtAngstrom angstrom_,
        PoolGate gate_,
        PoolId id_,
        PoolId refId_,
        MockERC20 asset0_,
        MockERC20 asset1_,
        address gov
    ) {
        uniV4 = uniV4_;
        angstrom = angstrom_;
        gate = gate_;
        id = id_;
        refId = refId_;
        asset0 = asset0_;
        asset1 = asset1_;

        vm.prank(rewarder.addr);
        asset0.approve(address(angstrom), type(uint256).max);
        vm.prank(rewarder.addr);
        asset1.approve(address(angstrom), type(uint256).max);

        {
            address[] memory newNodes = new address[](1);
            newNodes[0] = rewarder.addr;
            vm.prank(gov);
            angstrom.govToggleNodes(newNodes);
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

    function debug_showGrowths() public view {
        int256[] memory initialized = _ghost_initializedTicks.values();
        LibSort.sort(initialized);
        (uint160 currentPrice, int24 reportedCurrent,,,,,) = uniV4.getPool(id);
        int24 currentTick = TickMath.getTickAtSqrtPrice(currentPrice).normalize();
        console.log("global growth [%s]: %s", reportedCurrent.toStr(), angstrom.globalGrowthOutside(id).fmtD(12));
        for (uint256 i = 0; i < initialized.length; i++) {
            int24 tick = int24(initialized[i]);
            string memory marker = tick == currentTick ? "> " : "  ";
            console.log("%s%s: %s", marker, tick.toStr().lpad(" ", 4), angstrom.rewardGrowthOutside(id, tick).fmtD(12));
        }
    }

    function ghost_positions() public view returns (Position[] memory) {
        return _ghost_postitions;
    }

    function addLiquidity(int24 lowerTick, int24 upperTick, uint256 liquidity) public {
        if (TEST_LOGS) {
            console.log("addLiquidity(%s, %s, %s)", lowerTick.toStr(), upperTick.toStr(), liquidity.fmtD(6, 21));
        }
        assertGt(liquidity, 0, "Liquidity zero");
        assertLt(lowerTick, upperTick, "Upper tick below or equal to lower tick");
        assertEq(lowerTick % TICK_SPACING, 0, "Lower tick incorrectly spaced");
        assertEq(upperTick % TICK_SPACING, 0, "Lower tick incorrectly spaced");
        gate.setHook(address(angstrom));
        gate.addLiquidity(address(asset0), address(asset1), lowerTick, upperTick, liquidity);
        gate.setHook(address(0));
        gate.addLiquidity(address(asset0), address(asset1), lowerTick, upperTick, liquidity);
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
        targetSqrtPrice =
            uint160(bound(targetSqrtPrice, TickMath.getSqrtPriceAtTick(lowest), TickMath.getSqrtPriceAtTick(highest)));
        if (TEST_LOGS) {
            console.log("swapToPrice(%s) [%s]", targetSqrtPrice, TickMath.getTickAtSqrtPrice(targetSqrtPrice).toStr());
        }

        _swapTo(targetSqrtPrice);
        if (TEST_LOGS) console.log("");
    }

    function swapToBoundary(uint256 tickIndex) public passesTime {
        tickIndex = bound(tickIndex, 0, _ghost_initializedTicks.length() - 1);
        int24 targetTick = int24(_ghost_initializedTicks.at(tickIndex));
        if (TEST_LOGS) console.log("swapToBoundary(%s)", targetTick.toStr());
        uint160 targetSqrtPrice = TickMath.getSqrtPriceAtTick(targetTick);
        _swapTo(targetSqrtPrice);

        if (TEST_LOGS) console.log("");
    }

    function _swapTo(uint160 targetSqrtPrice) internal {
        if (TEST_LOGS) debug_showGrowths();

        (uint160 currentPrice,,,,,,) = uniV4.getPool(id);

        vm.assume(targetSqrtPrice != currentPrice);

        // vm.assume(targetSqrtPrice != currentPrice);

        bool zeroForOne = targetSqrtPrice < currentPrice;
        (MockERC20 assetIn, MockERC20 assetOut) = zeroForOne ? (asset0, asset1) : (asset1, asset0);

        gate.setHook(address(0));
        // Do initial swap to price to get delta.
        uint256 snapshot = vm.snapshot();
        _debug_logRefPool();
        BalanceDelta delta = gate.swap(address(assetIn), address(assetOut), type(int256).min, targetSqrtPrice);
        if (delta.amount0() == 0 && delta.amount1() == 0) {
            require(vm.revertTo(snapshot), "failed to revert");
            return;
        }
        _debug_logRefPool();
        // Swap back to original price.
        gate.swap(address(assetOut), address(assetIn), type(int256).min, currentPrice);
        _debug_logRefPool();
        delta = gate.swap(
            address(assetIn),
            address(assetOut),
            zeroForOne ? delta.amount0() : delta.amount1(),
            zeroForOne ? MIN_SQRT_RATIO : MAX_SQRT_RATIO
        );
        _debug_logRefPool();

        Bundle memory bundle =
            zeroForOne ? _newBundle(uint128(-delta.amount0()), 0) : _newBundle(0, uint128(-delta.amount1()));

        bundle.poolUpdates = new PoolUpdate[](1);
        PoolUpdate memory poolUpdate = bundle.poolUpdates[0];
        poolUpdate.assetIn = address(assetIn);
        poolUpdate.assetOut = address(assetOut);
        poolUpdate.amountIn = zeroForOne ? uint128(-delta.amount0()) : uint128(-delta.amount1());
        bundle.addDeltas(0, 1, delta);

        {
            (, int24 currentTick,,,,, uint128 liquidity) = uniV4.getPool(refId);
            uint128[] memory quantities = new uint128[](1);

            poolUpdate.rewardUpdate = RewardsUpdate({
                below: true,
                startTick: currentTick + 1,
                startLiquidity: liquidity,
                quantities: quantities
            });
        }

        vm.prank(rewarder.addr);
        bytes memory encoded = bundle.encode();
        angstrom.execute(encoded);

        if (TEST_LOGS) debug_showGrowths();
    }

    function _debug_poolUpdate(PoolUpdate memory) public pure {}

    function _debug_logRefPool() internal view {
        (uint160 currentPrice, int24 tick,,,,, uint128 liquidity) = uniV4.getPool(refId);
        console.log("ref pool");
        console.log("  currentPrice: %s", currentPrice);
        console.log("  tick: %s", tick.toStr());
        console.log("  liquidity: %s", liquidity.fmtD(12, 21));
    }

    function rewardLiquidity(uint256 ticksToReward, PRNG memory rng) public passesTime {
        uint256 totalTicks = _ghost_liquidInitializedTicks.length();
        ticksToReward = bound(ticksToReward, 0, totalTicks);

        // Select ticks & amounts to reward with.
        uint128 total = 0;
        UsedIndexMap memory map;
        map.init(totalTicks, totalTicks / 4);
        TickReward[] memory rewards = new TickReward[](ticksToReward);
        for (uint256 i = 0; i < ticksToReward; i++) {
            int24 tick = int24(_ghost_liquidInitializedTicks.at(rng.useRandIndex(map)));
            uint128 amount = u128(rng.randchoice(0.1e18, 0, rng.randmag(0.01e18, 100.0e18)));
            total += amount;
            _ghost_tickRewards.push(rewards[i] = TickReward({tick: tick, amount: amount}));
        }

        if (TEST_LOGS) {
            string memory rewardStr = "";
            for (uint256 i = 0; i < rewards.length; i++) {
                TickReward memory reward = rewards[i];
                if (i > 0) rewardStr = string.concat(rewardStr, ", ");
                rewardStr = string.concat(rewardStr, "(", reward.tick.toStr(), ", ", reward.amount.fmtD(18), ")");
            }
            console.log("rewardLiquidity([%s])", rewardStr);
            debug_showGrowths();
        }

        RewardsUpdate[] memory rewardUpdates = rewards.toUpdates(uniV4, id);
        Bundle memory bundle = _newBundle(total, 0);
        bundle.poolUpdates = new PoolUpdate[](rewardUpdates.length);

        for (uint256 i = 0; i < rewardUpdates.length; i++) {
            PoolUpdate memory poolUpdate = bundle.poolUpdates[i];
            poolUpdate.assetIn = address(asset0);
            poolUpdate.assetOut = address(asset1);
            poolUpdate.rewardUpdate = rewardUpdates[i];
            this._debug_poolUpdate(poolUpdate);
        }

        vm.prank(rewarder.addr);
        bytes memory encoded = bundle.encode();
        angstrom.execute(encoded);

        if (TEST_LOGS) debug_showGrowths();
        if (TEST_LOGS) console.log("");
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
        bundle.assets[0].addr = address(asset0);
        bundle.assets[1].addr = address(asset1);
        uint256 length = (amount0 > 0).into() + (amount1 > 0).into();
        bundle.toBOrders = new TopOfBlockOrder[](length);
        if (amount0 > 0) {
            asset0.mint(rewarder.addr, amount0);
            TopOfBlockOrder memory tob = bundle.toBOrders[0];
            tob.quantityIn = amount0;
            _fillAndSign(tob, true);
        }
        if (amount1 > 0) {
            asset1.mint(rewarder.addr, amount1);
            TopOfBlockOrder memory tob = bundle.toBOrders[(amount0 > 0).into()];
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
