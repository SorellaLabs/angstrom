// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "test/_helpers/BaseTest.sol";

import {SafeTransferLib} from "solady/src/utils/SafeTransferLib.sol";
import {UniV4Inspector} from "test/_view-ext/UniV4Inspector.sol";
import {RouterActor} from "test/_mocks/RouterActor.sol";
import {OpenAngstrom} from "test/_mocks/OpenAngstrom.sol";
import {MockERC20} from "super-sol/mocks/MockERC20.sol";
import {
    AngstromHandler,
    Env,
    LiquidityPosition,
    LiquidityAdd,
    PositionKey,
    TickReward
} from "./AngstromHandler.sol";
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

    uint256 internal constant REWARD_DISCREP_THRESHOLD = 5;

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
        selectors.push(AngstromHandler.removeLiquidity.selector);

        targetSelector(FuzzSelector({addr: address(handler), selectors: selectors}));
        targetContract(address(handler));
    }

    // function invariant_bundleSolvency() public view {
    //     address[] memory assets = handler.enabledAssets();
    //     for (uint256 i = 0; i < assets.length; i++) {
    //         address asset = assets[i];
    //         int256 delta = handler.ghost_netSavedDeltas(asset);
    //         uint256 balance = asset.balanceOf(address(e.angstrom));
    //         uint256 lpRewards = handler.ghost_pendingLpRewards(asset);
    //         uint256 deposits = handler.ghost_totalDeposits(asset);

    //         if (delta >= 0) {
    //             assertEq(
    //                 balance,
    //                 deposits + lpRewards + uint256(delta),
    //                 string.concat("delta: ", delta.toStr())
    //             );
    //         } else {
    //             uint256 change;
    //             unchecked {
    //                 change = uint256(-delta);
    //             }
    //             assertEq(
    //                 balance, deposits + lpRewards - change, string.concat("delta: ", delta.toStr())
    //             );
    //         }
    //     }
    // }

    function invariant_correctRewardAttribution() public view {
        _invariant_correctRewardAttribution(0);
    }

    function _invariant_correctRewardAttribution(uint256 poolIndex) internal view {
        PositionKey[] memory keys = handler.positionKeys(poolIndex);

        PoolId id = handler.poolIndexToId(poolIndex);
        uint256 totalRewards = 0;
        // Check position totals
        if (DEBUG) console.log("[position totals]");
        for (uint256 i = 0; i < keys.length; i++) {
            PositionKey k = keys[i];
            LiquidityPosition memory position = handler.getPosition(poolIndex, k);
            uint256 expectedRewards = (position.totalRewardsX128 >> 128) - position.claimedRewards;
            uint256 positionRewards = e.angstrom.getPositionRewards(
                id, address(position.owner), position.lowerTick, position.upperTick, bytes32(0)
            );
            if (DEBUG) {
                console.log("%s:", i);
                console.log(
                    "  pos.range: (%s, %s)", position.lowerTick.toStr(), position.upperTick.toStr()
                );
                console.log("  pos.liquidity: %s", position.totalLiquidity);
                console.log("  positionRewards: %s", positionRewards);
                console.log("  expectedRewards: %s", expectedRewards);
                console.log("  adds:");
                for (uint256 j = 0; j < position.adds.length; j++) {
                    LiquidityAdd memory add = position.adds[j];
                    console.log("    %s:", j);
                    console.log("      liquidity: %s", add.liquidity);
                    add.rewardEndIndex == type(uint256).max
                        ? console.log("      rewards: (%s..)", add.rewardStartIndex)
                        : console.log(
                            "      rewards: (%s..%s)", add.rewardStartIndex, add.rewardEndIndex
                        );
                }
            }
            assertLe(positionRewards, expectedRewards, "Rewards should never exceed expected");
            assertApproxEqAbs(positionRewards, expectedRewards, REWARD_DISCREP_THRESHOLD);
            totalRewards += positionRewards;
        }

        (address asset,,) = handler.getPool(poolIndex);
        uint256 total = handler.ghost_totalLpRewards(asset);
        uint256 claimed = handler.ghost_claimedLpRewards(asset);
        uint256 unclaimable = handler.ghost_unclaimableRewards(asset);

        if (DEBUG) {
            console.log("[total rewards (%s)]", poolIndex);
            console.log("  totalRewards: %s", totalRewards);
            console.log("  unclaimable: %s", unclaimable);
            console.log("  total: %s", total);
            console.log("  claimed: %s", claimed);
        }

        assertLe(totalRewards + claimed + unclaimable, total);
        assertApproxEqAbs(
            totalRewards + claimed, total - unclaimable, REWARD_DISCREP_THRESHOLD * keys.length
        );
    }

    // function invariant_ghost_totalDepositsConsistency() public view {
    //     address[] memory assets = handler.enabledAssets();
    //     address[] memory actors = handler.actors();
    //     for (uint256 i = 0; i < assets.length; i++) {
    //         address asset = assets[i];
    //         uint256 assumedTotal = handler.ghost_totalDeposits(asset);
    //         uint256 realBalance = 0;
    //         for (uint256 j = 0; j < actors.length; j++) {
    //             realBalance += e.angstrom.balanceOf(asset, actors[j]);
    //         }
    //         assertEq(realBalance, assumedTotal);
    //     }
    // }

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
