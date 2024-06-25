// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {BaseTest} from "./helpers/BaseTest.sol";
import {Angstrom} from "../src/Angstrom.sol";
import {HelpingAngstrom} from "./helpers/HelpingAngstrom.sol";
import {MockERC20} from "./mocks/MockERC20.sol";

// import {OrderType, CoreLimitOrder, PartialOrder, Side, AssetType} from "../src/interfaces/OrderTypes.sol";
// import {GlobalContext, Pair} from "../src/libraries/GlobalContext.sol";
import {console2 as console} from "forge-std/console2.sol";

/// @author philogy <https://github.com/philogy>
contract AngstromTest is BaseTest {
// HelpingAngstrom ang = new HelpingAngstrom(address(0));

// MockERC20 token0 = new MockERC20();
// MockERC20 token1 = new MockERC20();

// function test_partial() public {
//     console.log("token0: %s", address(token0));
//     console.log("token1: %s", address(token1));

//     Pair[] memory pairs = new Pair[](1);
//     pairs[0] = Pair({asset0: address(token0), asset1: address(token1), price: 2.0e18});
//     GlobalContext memory ctx = GlobalContext({pairs: pairs});

//     Angstrom.GenericOrder[] memory orders = new Angstrom.GenericOrder[](1);

//     Account memory owner = makeAccount("owner");

//     PartialOrder memory porder;
//     porder.filled = 1e18;
//     CoreLimitOrder memory order = porder.core;
//     order.side = Side.Bid;
//     order.inType = AssetType.Liquid;
//     order.outType = AssetType.Angstrom;
//     order.asset0 = address(token0);
//     order.asset1 = address(token1);
//     order.amount = 1.2e18;
//     order.owner = owner.addr;

//     order.signature = sign(owner, ang.hashTyped(order.hashAsPartial()));

//     orders[0] = Angstrom.GenericOrder({otype: OrderType.Partial, data: abi.encode(porder)});

//     ang.execute(ctx, orders, "");
// }

// function test_all_exact_out() public {
//     Pair[] memory pairs = new Pair[](1);
//     pairs[0] = Pair({asset0: address(token0), asset1: address(token1), price: 3.0e18});
//     GlobalContext memory ctx = GlobalContext({pairs: pairs});

//     Angstrom.GenericOrder[] memory orders = new Angstrom.GenericOrder[](1);

//     Account memory owner = makeAccount("owner");

//     CoreLimitOrder memory order;
//     order.side = Side.Ask;
//     order.inType = AssetType.Liquid;
//     order.outType = AssetType.Angstrom;
//     order.asset0 = address(token0);
//     order.asset1 = address(token1);
//     order.limit = 0.4e18;
//     order.amount = 1e18;
//     order.owner = owner.addr;
//     order.signature = sign(owner, ang.hashTyped(order.hashAsExactOut()));

//     orders[0] = Angstrom.GenericOrder({otype: OrderType.AllExactOut, data: abi.encode(order)});

//     ang.execute(ctx, orders, "");
// }
}
