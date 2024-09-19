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

import {Bundle} from "src/reference/Bundle.sol";
import {Asset} from "src/reference/Asset.sol";
import {PoolUpdate, RewardsUpdate} from "src/reference/PoolUpdate.sol";
import {TopOfBlockOrder} from "src/reference/OrderTypes.sol";

import {EnumerableSetLib} from "solady/src/utils/EnumerableSetLib.sol";
import {console} from "forge-std/console.sol";
import {FormatLib} from "super-sol/libraries/FormatLib.sol";

/// @author philogy <https://github.com/philogy>
contract PoolRewardsHandler is BaseTest {
    using FormatLib for *;
    using EnumerableSetLib for EnumerableSetLib.Int256Set;

    using RewardLib for TickReward[];

    UniV4Inspector public immutable uniV4;
    ExtAngstrom public immutable angstrom;
    PoolGate public immutable gate;
    PoolId public immutable id;

    MockERC20 public immutable asset0;
    MockERC20 public immutable asset1;
    Account public rewarder = makeAccount("rewarder");

    constructor(
        UniV4Inspector uniV4_,
        ExtAngstrom angstrom_,
        PoolGate gate_,
        PoolId id_,
        MockERC20 asset0_,
        MockERC20 asset1_,
        address gov
    ) {
        uniV4 = uniV4_;
        angstrom = angstrom_;
        gate = gate_;
        id = id_;
        asset0 = asset0_;
        asset1 = asset1_;

        vm.prank(rewarder.addr);
        asset0.approve(address(angstrom), type(uint256).max);

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

    function addLiquidity(int24 lowerTick, int24 upperTick, uint256 liquidity) public {
        assertGt(liquidity, 0, "Liquidity zero");
        assertLt(lowerTick, upperTick, "Upper tick below or equal to lower tick");
        assertEq(lowerTick % TICK_SPACING, 0, "Lower tick incorrectly spaced");
        assertEq(upperTick % TICK_SPACING, 0, "Lower tick incorrectly spaced");
        gate.addLiquidity(address(asset0), address(asset1), lowerTick, upperTick, liquidity);
        if (_ghost_initializedTicks.add(lowerTick)) {}
        if (_ghost_initializedTicks.add(upperTick)) {}
        _ghost_postitions.push(Position(lowerTick, upperTick, liquidity));

        for (int24 tick = lowerTick; tick < upperTick; tick += TICK_SPACING) {
            ghost_liquidityAtTick[tick] += liquidity;
        }
    }

    function r() public view returns (address) {
        return rewarder.addr;
    }

    TickReward[] _ghost_tickRewards;

    function ghost_tickRewards() public view returns (TickReward[] memory) {
        return _ghost_tickRewards;
    }

    function rewardLiquidity(uint256 ticksToReward, PRNG memory rng) public {
        uint256 totalTicks = _ghost_initializedTicks.length();
        ticksToReward = bound(ticksToReward, 0, totalTicks);

        // Select ticks & amounts to reward with.
        uint128 total = 0;
        UsedIndexMap memory map;
        map.init(totalTicks, totalTicks / 4);
        TickReward[] memory rewards = new TickReward[](ticksToReward);
        for (uint256 i = 0; i < ticksToReward; i++) {
            int24 tick = int24(_ghost_initializedTicks.at(rng.useRandIndex(map)));
            uint128 amount = u128(rng.randmag(0.8e18, 100.0e18));
            total += amount;
            _ghost_tickRewards.push(rewards[i] = TickReward({tick: tick, amount: amount}));
        }

        vm.prank(msg.sender);
        asset0.mint(rewarder.addr, total);

        RewardsUpdate[] memory rewardUpdates = rewards.toUpdates(uniV4, id);
        Bundle memory bundle;
        bundle.assets = new Asset[](2);
        bundle.assets[0].addr = address(asset0);
        bundle.assets[1].addr = address(asset1);
        bundle.poolUpdates = new PoolUpdate[](rewardUpdates.length);
        bundle.toBOrders = new TopOfBlockOrder[](1);
        TopOfBlockOrder memory tob = bundle.toBOrders[0];

        vm.roll(angstrom.lastBlockUpdated() + 1);

        for (uint256 i = 0; i < rewardUpdates.length; i++) {
            PoolUpdate memory poolUpdate = bundle.poolUpdates[i];

            poolUpdate.assetIn = address(asset0);
            poolUpdate.assetOut = address(asset1);
            RewardsUpdate memory rewardUpdate = rewardUpdates[i];
            poolUpdate.rewardUpdate = rewardUpdate;

            for (uint256 j = 0; j < rewardUpdate.quantities.length; j++) {
                tob.quantityIn += rewardUpdate.quantities[j];
            }
        }

        tob.assetIn = address(asset0);
        tob.assetOut = address(asset1);
        tob.validForBlock = u64(block.number);
        sign(rewarder, tob.meta, angstrom.hashTyped(tob.hash()));

        vm.prank(rewarder.addr);
        bytes memory encoded = bundle.encode();
        angstrom.execute(encoded);
    }

    function ghost_totalInititalizedTicks() public view returns (uint256) {
        return _ghost_initializedTicks.length();
    }
}
