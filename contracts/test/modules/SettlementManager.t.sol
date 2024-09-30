// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "test/_helpers/BaseTest.sol";
import {PoolManager} from "v4-core/src/PoolManager.sol";
import {Bundle, TopOfBlockOrder, Asset} from "src/reference/Bundle.sol";
import {MockERC20} from "super-sol/mocks/MockERC20.sol";
import {Angstrom} from "src/Angstrom.sol";
import {LibSort} from "solady/src/utils/LibSort.sol";

/// @author philogy <https://github.com/philogy>
contract SettlementManagerTest is BaseTest {
    Angstrom angstrom;
    address controller = makeAddr("controller");
    address feeMaster = makeAddr("fee_master");
    address validator = makeAddr("validator");
    Account searcher = makeAccount("searcher");
    PoolManager uniV4;

    event AngstromFeeSummary(bytes32 summaryHash) anonymous;

    address[] assets;
    address otherAsset;
    bytes32 domainSeparator;

    function setUp() public {
        uniV4 = new PoolManager();
        angstrom =
            Angstrom(deployAngstrom(type(Angstrom).creationCode, uniV4, controller, feeMaster));
        domainSeparator = computeDomainSeparator(address(angstrom));
        uint256 pairs = 40;
        address[] memory newAssets = new address[](pairs * 2);
        vm.startPrank(searcher.addr);
        for (uint256 i = 0; i < pairs; i++) {
            (address asset0, address asset1) = deployTokensSorted();
            newAssets[i * 2 + 0] = asset0;
            newAssets[i * 2 + 1] = asset1;
            MockERC20(asset0).approve(address(angstrom), type(uint256).max);
            MockERC20(asset1).approve(address(angstrom), type(uint256).max);
        }
        vm.stopPrank();
        LibSort.sort(newAssets);
        otherAsset = newAssets[pairs * 2 - 1];
        assets = newAssets;
        assets.pop();

        vm.prank(controller);
        angstrom.toggleNodes(addrs(abi.encode(validator)));
    }

    function test_single() public {
        Bundle memory bundle;
        address asset = assets[3];
        uint128 amount = 24.27e18;
        addFee(bundle, asset, amount);
        enablePool(asset, otherAsset);

        bytes memory payload = bundle.encode(rawGetConfigStore(address(angstrom)));
        vm.expectEmitAnonymous(address(angstrom));
        emit AngstromFeeSummary(bundle.feeSummary());
        vm.prank(validator);
        angstrom.execute(payload);

        // Pull fee.
        assertEq(MockERC20(asset).balanceOf(feeMaster), 0);
        vm.prank(feeMaster);
        angstrom.pullFee(asset, amount);
        assertEq(MockERC20(asset).balanceOf(feeMaster), amount);
    }

    function enablePool(address asset0, address asset1) internal {
        vm.prank(controller);
        angstrom.configurePool(asset0, asset1, 60, 0);
    }

    function addFee(Bundle memory bundle, address assetAddr, uint128 amount) internal {
        MockERC20(assetAddr).mint(searcher.addr, amount);

        TopOfBlockOrder memory tob;
        tob.assetIn = assetAddr;
        tob.assetOut = otherAsset;
        tob.quantityIn = amount;
        tob.validForBlock = uint64(block.number);
        sign(searcher, tob, domainSeparator);
        bundle.addToB(tob);

        Asset memory asset = bundle.getAsset(assetAddr);
        asset.save += amount;
    }
}
