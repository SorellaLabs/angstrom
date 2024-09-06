// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {Test} from "forge-std/Test.sol";
import {MixedSignLib} from "../../src/libraries/MixedSignLib.sol";

/// @author philogy <https://github.com/philogy>
contract MixedSignTest is Test {
    function test_fuzzing_add(uint128 x, int128 y) public {
        uint128 absY = _abs128(y);

        if (y < 0 && absY > x) {
            vm.expectRevert(MixedSignLib.ArithmeticOverflowUnderflow.selector);
            MixedSignLib.add(x, y);
        } else if (y > 0 && absY > type(uint128).max - x) {
            vm.expectRevert(MixedSignLib.ArithmeticOverflowUnderflow.selector);
            MixedSignLib.add(x, y);
        } else {
            uint128 z = MixedSignLib.add(x, y);

            if (y > 0) assertEq(z, x + absY);
            else assertEq(z, x - absY);
        }
    }

    function test_fuzzing_sub(uint128 x, int128 y) public {
        uint128 absY = _abs128(y);

        if (y < 0 && absY > type(uint128).max - x) {
            vm.expectRevert(MixedSignLib.ArithmeticOverflowUnderflow.selector);
            MixedSignLib.sub(x, y);
        } else if (y > 0 && absY > x) {
            vm.expectRevert(MixedSignLib.ArithmeticOverflowUnderflow.selector);
            MixedSignLib.sub(x, y);
        } else {
            uint128 z = MixedSignLib.sub(x, y);

            if (y > 0) assertEq(z, x - absY, "positive y");
            else assertEq(z, x + absY, "negative y");
        }
    }

    function _abs128(int128 x) internal pure returns (uint128 y) {
        unchecked {
            y = x < 0 ? uint128(-x) : uint128(x);
        }
    }
}
