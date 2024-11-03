// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "test/_helpers/BaseTest.sol";

import {SafeTransferLib} from "solady/src/utils/SafeTransferLib.sol";
import {UniV4Inspector} from "test/_view-ext/UniV4Inspector.sol";
import {RouterActor} from "test/_mocks/RouterActor.sol";
import {OpenAngstrom} from "test/_mocks/OpenAngstrom.sol";
import {MockERC20} from "super-sol/mocks/MockERC20.sol";
import {AngstromHandler, Env, LiquidityAdd, AddKey, TickReward} from "./AngstromHandler.sol";
import {tuint256} from "transient-goodies/TransientPrimitives.sol";
import {FixedPointMathLib} from "solady/src/utils/FixedPointMathLib.sol";
import {IUniV4, IPoolManager} from "src/interfaces/IUniV4.sol";
import {PoolId} from "v4-core/src/types/PoolId.sol";

import {TickMath} from "v4-core/src/libraries/TickMath.sol";

import {FormatLib} from "super-sol/libraries/FormatLib.sol";
import {console} from "forge-std/console.sol";

/// @author philogy <https://github.com/philogy>
contract AngstromInvariantsTest is BaseTest {
    using IUniV4 for IPoolManager;
    using FixedPointMathLib for uint256;
    using SafeTransferLib for *;
    using FormatLib for *;

    uint256 internal TOTAL_ASSETS = 40;

    Env e;
    AngstromHandler handler;

    bytes4[] selectors;

    function setUp() public {
        e.owner = makeAddr("owner");
        e.controller = makeAddr("controller");
        e.feeMaster = makeAddr("feeMaster");

        vm.prank(e.owner);
        e.uniV4 = new UniV4Inspector();
        e.angstrom =
            OpenAngstrom(deployAngstrom(type(OpenAngstrom).creationCode, e.uniV4, e.controller));
        e.assets = _fillAssets(new MockERC20[](TOTAL_ASSETS));
        e.mirrors = _fillAssets(new MockERC20[](TOTAL_ASSETS));

        handler = new AngstromHandler(e);

        for (uint256 i = 0; i < e.assets.length; i++) {
            e.assets[i].mint(address(handler), type(uint128).max);
        }
        for (uint256 i = 0; i < e.mirrors.length; i++) {
            e.mirrors[i].mint(address(handler), type(uint128).max);
        }

        handler.initializePool(0, 1, 60, 0.002e6, TickMath.getSqrtPriceAtTick(0));

        selectors.push(AngstromHandler.addLiquidity.selector);
        selectors.push(AngstromHandler.rewardTicks.selector);

        targetSelector(FuzzSelector({addr: address(handler), selectors: selectors}));
        targetContract(address(handler));
    }

    function invariant_bundleSolvency() public view {
        address[] memory assets = handler.enabledAssets();
        for (uint256 i = 0; i < assets.length; i++) {
            address asset = assets[i];
            int256 delta = handler.ghost_netSavedDeltas(asset);
            uint256 balance = asset.balanceOf(address(e.angstrom));
            uint256 lpRewards = handler.ghost_pendingLpRewards(asset);
            uint256 deposits = handler.ghost_totalDeposits(asset);

            if (delta >= 0) {
                assertEq(
                    balance,
                    deposits + lpRewards + uint256(delta),
                    string.concat("delta: ", delta.toStr())
                );
            } else {
                uint256 change;
                unchecked {
                    change = uint256(-delta);
                }
                assertEq(
                    balance, deposits + lpRewards - change, string.concat("delta: ", delta.toStr())
                );
            }
        }
    }

    function invariant_correctRewardAttribution() public {
        _invariant_correctRewardAttribution(0);
    }

    mapping(AddKey => tuint256) _positionTotals;
    mapping(AddKey => tuint256) _positionAccounted;

    function _invariant_correctRewardAttribution(uint256 poolIndex) internal {
        LiquidityAdd[] memory adds = handler.liquidityAdds(poolIndex);
        TickReward[] memory rewards = handler.tickRewards(poolIndex);
        PoolId id = handler.poolIndexToId(poolIndex);
        bool[] memory claimable = new bool[](rewards.length);

        // Compute liquidity laying claim to any individual reward.
        uint256[] memory liquidityClaiming = new uint256[](rewards.length);
        for (uint256 i = 0; i < adds.length; i++) {
            LiquidityAdd memory add = adds[i];
            uint256 endIndex = min(rewards.length, add.rewardEndIndex);
            for (uint256 j = add.rewardStartIndex; j < endIndex; j++) {
                TickReward memory reward = rewards[j];
                if (add.lowerTick <= reward.tick && reward.tick < add.upperTick) {
                    claimable[j] = true;
                    liquidityClaiming[j] += add.liquidity;
                }
            }
            _positionTotals[add.key()].set(0);
        }

        // Compute position totals
        for (uint256 i = 0; i < adds.length; i++) {
            LiquidityAdd memory add = adds[i];
            uint256 endIndex = min(rewards.length, add.rewardEndIndex);
            for (uint256 j = add.rewardStartIndex; j < endIndex; j++) {
                TickReward memory reward = rewards[j];
                if (add.lowerTick <= reward.tick && reward.tick < add.upperTick) {
                    uint256 claiming = liquidityClaiming[j];
                    _positionTotals[add.key()].inc(
                        (reward.amount * add.liquidity).fullMulDiv(1 << 128, claiming)
                            - add.claimedRewards
                    );
                }
            }
        }

        uint256 totalRewards = 0;
        // Check position totals
        if (DEBUG) console.log("[position totals]");
        for (uint256 i = 0; i < adds.length; i++) {
            LiquidityAdd memory add = adds[i];
            AddKey k = add.key();
            uint256 expectedRewardsWad = _positionTotals[k].get();
            if (_positionAccounted[k].get() == 1) {
                continue;
            } else {
                _positionAccounted[k].set(1);
            }
            uint256 expectedRewards = expectedRewardsWad >> 128;
            uint256 positionRewards = e.angstrom.getPositionRewards(
                id, address(add.owner), add.lowerTick, add.upperTick, bytes32(0)
            );
            if (DEBUG) {
                console.log("%s:", i);
                console.log("  add.range: (%s, %s)", add.lowerTick.toStr(), add.upperTick.toStr());
                console.log("  add.liquidity: %s", add.liquidity);
                console.log("  add.rewardStartIndex: %s", add.rewardStartIndex);
                console.log("  positionRewards: %s", positionRewards);
                console.log("  expectedRewards: %s", expectedRewards);
            }
            assertLe(positionRewards, expectedRewards, "Rewards should never exceed expected");
            assertApproxEqAbs(positionRewards, expectedRewards, 5);
            totalRewards += expectedRewards;
        }

        if (DEBUG) {
            console.log("[claimable]");
            for (uint256 i = 0; i < claimable.length; i++) {
                console.log("  %s: %s", i, claimable[i].toStr());
            }
        }

        uint256 totalUnclaimable = 0;
        for (uint256 i = 0; i < rewards.length; i++) {
            if (!claimable[i]) {
                totalUnclaimable += rewards[i].amount;
            }
        }

        if (DEBUG) console.log("[pending - unclaimable = total]");

        (address asset,,) = handler.getPool(poolIndex);
        uint256 totalPending = handler.ghost_pendingLpRewards(asset);
        uint256 totalRight = totalPending - totalUnclaimable;
        if (DEBUG) {
            console.log("  totalRewards: %s", totalRewards);
            console.log("  totalPending: %s", totalPending);
            console.log("  totalUnclaimable: %s", totalUnclaimable);
            console.log("    totalRight: %s", totalRight);
            console.log(
                "  rewardByAsset - totalRight: %s",
                (int256(totalRewards) - int256(totalRight)).toStr()
            );
        }
        assertApproxEqAbs(totalRewards, totalRight, 3 * adds.length);
    }

    function invariant_ghost_totalDepositsConsistency() public view {
        address[] memory assets = handler.enabledAssets();
        address[] memory actors = handler.actors();
        for (uint256 i = 0; i < assets.length; i++) {
            address asset = assets[i];
            uint256 assumedTotal = handler.ghost_totalDeposits(asset);
            uint256 realBalance = 0;
            for (uint256 j = 0; j < actors.length; j++) {
                realBalance += e.angstrom.balanceOf(asset, actors[j]);
            }
            assertEq(realBalance, assumedTotal);
        }
    }

    function _fillAssets(MockERC20[] memory assets) internal returns (MockERC20[] memory) {
        for (uint256 i = 0; i < assets.length; i++) {
            MockERC20 newAsset = new MockERC20();
            for (uint256 j = 0; j < i; j++) {
                if (newAsset < assets[j]) {
                    (newAsset, assets[j]) = (assets[j], newAsset);
                }
            }
            assets[i] = newAsset;
        }
        return assets;
    }
}
