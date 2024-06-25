// SPDX-License-Identifier: MIT
pragma solidity ^0.8.4;

import {Test} from "forge-std/Test.sol";
import {PriceGraphLib, PriceGraph, AssetIndex} from "src/libraries/PriceGraph.sol";

/// @author philogy <https://github.com/philogy>
contract PriceGraphTest is Test {
    function test_fuzzing_allocGraph(uint256 width) public {
        width = bound(width, 0, PriceGraphLib.MAX_WIDTH);
        PriceGraph graph = PriceGraphLib.init(width);
        assertEq(graph.width(), width);
    }

    function test_fuzzing_uninitializedCellsRevertUponRetrieval(uint256 width) public {
        width = bound(width, 0, PriceGraphLib.MAX_WIDTH);
        PriceGraph graph = PriceGraphLib.init(width);
        for (uint256 i = 1; i < width; i++) {
            for (uint256 j = 0; j < i; j++) {
                vm.expectRevert(PriceGraphLib.PriceNotSet.selector);
                graph.get(AssetIndex.wrap(uint16(i)), AssetIndex.wrap(uint16(j)));
            }
        }
    }

    function test_fuzzing_canSetPriceOnce(uint256 width, uint256 fuzzed_i, uint256 fuzzed_j, uint256 price) public {
        width = bound(width, 2, PriceGraphLib.MAX_WIDTH);
        AssetIndex i = AssetIndex.wrap(uint16(bound(fuzzed_i, 1, width - 1)));
        AssetIndex j = AssetIndex.wrap(uint16(bound(fuzzed_j, 0, i.into() - 1)));
        PriceGraph graph = PriceGraphLib.init(width);
        graph.set(i, j, price);
        vm.expectRevert(PriceGraphLib.PriceAlreadySet.selector);
        graph.set(i, j, price);
        assertEq(graph.get(i, j), price);
    }
}
