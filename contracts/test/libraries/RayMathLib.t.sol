// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "../_helpers/BaseTest.sol";
import {RayMathLib} from "../../src/libraries/RayMathLib.sol";
import {stdError} from "forge-std/StdError.sol";
import {FixedPointMathLib} from "solady/src/utils/FixedPointMathLib.sol";

/// @author philogy <https://github.com/philogy>
contract RayMathLibTest is BaseTest {
    using RayMathLib for *;

    function test_fuzzing_mulRay_commutative(uint256 x, uint256 y) public view {
        (, bytes memory err1, uint256 z1) = tryMulRay(x, y);
        (, bytes memory err2, uint256 z2) = tryMulRay(y, x);

        assertEq(err1, err2, "different error");
        assertEq(z1, z2, "different result");
    }

    function test_mulRay() public pure {
        assertEq(0.mulRayDown(0), 0);
        assertEq(3.mulRayDown(0.5e27), 1);
        assertEq(1e18.mulRayDown(0.5e27), 0.5e18);
        assertEq(1.1e18.mulRayDown(3.5e27), 3.85e18);
        assertEq(6.0e4.mulRayDown(0.166666666666666666666666666e27), 0.9999e4);
    }

    function test_fuzzing_mulRay_zero(uint256 x) public pure {
        assertEq(0.mulRayDown(x), 0);
        assertEq(x.mulRayDown(0), 0);
    }

    function test_fuzzing_invRay_divOne_equivalence(uint256 x) public pure {
        x = bound(x, 1, type(uint256).max);
        uint256 y1 = RayMathLib.RAY.divRayDown(x);
        uint256 y2 = x.invRayUnchecked();
        assertEq(y1, y2);
    }

    function test_divRay() public pure {
        assertEq(3.divRayDown(0.5e27), 6);
        assertEq(3200.divRayDown(3.2e27), 1000);
        assertEq(1e18.divRayDown(uint256(1.0e27) / 7), 7.0e18);
        assertEq(34.287e18.divRayDown(1.00023879e27), 34.278814561870770878e18);
    }

    function test_fuzzing_divRay_prevents_divide_by_zero(uint256 x) public {
        x = bound(x, 0, type(uint256).max / RayMathLib.RAY);
        vm.expectRevert(stdError.divisionError);
        x.divRayDown(0);
    }

    function test_fuzzing_mulRay_prevents_overflow(uint256 x, uint256 y) public {
        x = bound(x, 2, type(uint256).max);
        y = bound(y, type(uint256).max / x + 1, type(uint256).max);
        vm.expectRevert(stdError.arithmeticError);
        x.mulRayDown(y);
    }

    function tryMulRay(uint256 x, uint256 y) internal view returns (bool, bytes memory, uint256) {
        return tryFn(this._extMulRay, x, y);
    }

    function tryDivRay(uint256 x, uint256 y) internal view returns (bool, bytes memory, uint256) {
        return tryFn(this._extDivRay, x, y);
    }

    function _extMulRay(uint256 x, uint256 y) external pure returns (uint256) {
        return x.mulRayDown(y);
    }

    function _extDivRay(uint256 x, uint256 y) external pure returns (uint256) {
        return x.divRayDown(y);
    }
}
