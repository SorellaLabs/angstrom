// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "test/_helpers/BaseTest.sol";
import {PositionFetcher, Position} from "../PositionFetcher.sol";
import {PoolId, PoolIdLibrary, PoolKey} from "v4-core/src/types/PoolId.sol";
import {IHooks} from "v4-core/src/interfaces/IHooks.sol";
import {
    PositionManager,
    PositionInfoLibrary,
    PositionInfo,
    IPoolManager,
    IAllowanceTransfer,
    IPositionDescriptor,
    IWETH9
} from "v4-periphery/src/PositionManager.sol";

import {console} from "forge-std/console.sol";
import {FormatLib} from "super-sol/libraries/FormatLib.sol";

/// @author philogy <https://github.com/philogy>
contract PositionFetcherTest is BaseTest {
    using FormatLib for *;
    using PoolIdLibrary for PoolKey;

    address angstrom = makeAddr("angstrom");
    address asset0 = makeAddr("asset0");
    address asset1 = makeAddr("asset1");
    address asset2 = makeAddr("asset2");

    MockPositionManager pos = new MockPositionManager();

    PoolKey pk1;
    PoolKey pk2;
    PoolKey pk3;
    PoolKey pk4;

    function setUp() public {
        pk1 = poolKey(asset0, asset1, 0);
        pk2 = poolKey(asset0, asset2, 0);
        pk3 = poolKey(asset0, asset1, 0);
        pk3.hooks = IHooks(angstrom);
        pk4 = poolKey(asset0, asset2, 0);
        pk4.hooks = IHooks(angstrom);
    }

    function test1() public {
        address user1 = makeAddr("user1");
        address user2 = makeAddr("user2");

        pos.mint(user1, -60, 60, pk1);
        pos.mint(user1, -100, 100, pk1);
        pos.mint(user2, 10, 100, pk4);
        pos.mint(user2, 10, 100, pk2);
        pos.mint(user1, -4000, -3400, pk3);
        pos.mint(user2, -660, -60, pk3);

        PositionFetcher fetcher = new PositionFetcher(pos, angstrom);

        (uint256 end, uint256 setEnd, Position[] memory positions) =
            fetcher.getPositions(user2, 1, 0, 1);

        console.log("end: %s", end);
        console.log("setEnd: %s", setEnd);
        console.log("positions.length: %s", positions.length);
        for (uint256 i = 0; i < positions.length; i++) {
            Position memory p = positions[i];
            console.log("i: %s", i);
            console.log("p.tokenId: %s", p.tokenId);
            console.log("tickLower: %s", p.tickLower.toStr());
            console.log("tickUpper: %s", p.tickUpper.toStr());
            console.logBytes25(p.poolId);
        }
    }
}

contract MockPositionManager is
    PositionManager(
        IPoolManager(address(0)),
        IAllowanceTransfer(address(0)),
        0,
        IPositionDescriptor(address(0)),
        IWETH9(address(0))
    )
{
    function mint(address to, int24 tickLower, int24 tickUpper, PoolKey calldata poolKey) public {
        uint256 tokenId;
        unchecked {
            tokenId = nextTokenId++;
        }
        _mint(to, tokenId);

        PositionInfo info = PositionInfoLibrary.initialize(poolKey, tickLower, tickUpper);
        positionInfo[tokenId] = info;

        bytes25 poolId = info.poolId();
        if (poolKeys[poolId].tickSpacing == 0) {
            poolKeys[poolId] = poolKey;
        }
    }
}
