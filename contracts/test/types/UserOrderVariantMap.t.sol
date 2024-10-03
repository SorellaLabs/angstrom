// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "test/_helpers/BaseTest.sol";
import {UserOrderVariantMap} from "src/types/UserOrderVariantMap.sol";

/// @author philogy <https://github.com/philogy>
contract OrderVariantMapTest is BaseTest {
    function setUp() public {}

    function test_fuzzing_oneBitPerProp(UserOrderVariantMap map, uint256 bitToFlip) public pure {
        bitToFlip = bound(bitToFlip, 0, 7);
        UserOrderVariantMap newMap = xor(map, uint8(1 << bitToFlip));

        bool[8] memory startProps = toProps(map);
        bool[8] memory newProps = toProps(newMap);

        uint256 totalChanges = 0;
        for (uint256 i = 0; i < 8; i++) {
            if (startProps[i] != newProps[i]) totalChanges += 1;
        }

        assertEq(totalChanges, 1);
    }

    function toProps(UserOrderVariantMap map) internal pure returns (bool[8] memory props) {
        props[0] = map.useInternal();
        props[1] = map.recipientIsSome();
        props[2] = map.noHook();
        props[3] = map.zeroForOne();
        props[4] = map.isStanding();
        props[5] = map.quantitiesPartial();
        props[6] = map.exactIn();
        props[7] = map.isEcdsa();
    }

    function xor(UserOrderVariantMap map, uint8 bit) internal pure returns (UserOrderVariantMap) {
        return UserOrderVariantMap.wrap(UserOrderVariantMap.unwrap(map) ^ bit);
    }
}
