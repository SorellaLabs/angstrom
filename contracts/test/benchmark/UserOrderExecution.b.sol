// SPDX-License-Identifier: BUSL-1.1
pragma solidity ^0.8.24;

import {BaseTest} from "../_helpers/BaseTest.sol";

import {ConversionLib} from "../../src/libraries/ConversionLib.sol";
import {UserOrder, UserOrderLib} from "../../src/reference/UserOrder.sol";
import {
    PartialStandingOrder,
    ExactStandingOrder,
    PartialFlashOrder,
    ExactFlashOrder,
    OrderMeta
} from "../../src/reference/OrderTypes.sol";
import {ExtAngstrom} from "../_view-ext/ExtAngstrom.sol";

import {PadeEncoded} from "../../src/types/PadeEncoded.sol";
import {ArrayLib} from "super-sol/libraries/ArrayLib.sol";
import {PoolId, PoolIdLibrary} from "v4-core/src/types/PoolId.sol";
import {UniV4Inspector} from "../_view-ext/UniV4Inspector.sol";
import {MockERC20} from "super-sol/mocks/MockERC20.sol";
import {HookDeployer} from "../_helpers/HookDeployer.sol";
import {FixedPointMathLib} from "solady/src/utils/FixedPointMathLib.sol";
import {RayMathLib} from "../../src/libraries/RayMathLib.sol";
import {PairIterator, PairItem, PairIteratorLib} from "./_helpers/PairIterator.sol";
import {TypedDataHasher, TypedDataHasherLib} from "src/types/TypedDataHasher.sol";
import {SafeCastLib} from "solady/src/utils/SafeCastLib.sol";
import {Bundle} from "src/reference/Bundle.sol";
import {Asset, AssetLib} from "src/reference/Asset.sol";
import {Pair, PairLib} from "src/reference/Pair.sol";
import {
    PriceAB,
    PriceAB,
    AmountA,
    AmountB,
    PriceAB as PriceOutVsIn,
    AmountA as AmountOut,
    AmountB as AmountIn
} from "src/types/Price.sol";

import {DEBUG_LOGS} from "src/modules/DevFlags.sol";

import {PoolGate} from "../_helpers/PoolGate.sol";
import {Trader} from "../_helpers/types/Trader.sol";
import {PRNG} from "super-sol/collections/PRNG.sol";
import {FormatLib} from "super-sol/libraries/FormatLib.sol";

import {console} from "forge-std/console.sol";

/// @author philogy <https://github.com/philogy>
contract UserOrderExecution is BaseTest, HookDeployer {
    using AssetLib for *;
    using PairLib for *;

    using ArrayLib for *;
    using FormatLib for *;
    using FixedPointMathLib for *;
    using RayMathLib for uint256;
    using SafeCastLib for *;

    address gov = makeAddr("gov");
    address owner = makeAddr("owner");
    address node = makeAddr("node");

    UniV4Inspector uniV4;
    PoolGate gate;
    ExtAngstrom angstrom;

    uint256 constant TOTAL_ASSETS = 4;
    uint256 constant TOTAL_PAIRS = TOTAL_ASSETS * (TOTAL_ASSETS - 1) / 2;
    address[] assets;

    uint256 constant TOTAL_ACTORS = 1_000;

    function setUp() public {
        uint256 expectedTotalPairs = 0;
        for (uint256 i = 0; i < TOTAL_ASSETS; i++) {
            expectedTotalPairs += i;
        }
        assertEq(TOTAL_PAIRS, expectedTotalPairs);
        for (uint256 i = 0; i < TOTAL_ASSETS; i++) {
            address newToken = address(new MockERC20());
            assets.push(newToken);
            for (uint256 j = 0; j < i; j++) {
                if (assets[j] > newToken) {
                    (assets[j], assets[i]) = (assets[i], assets[j]);
                    break;
                }
            }
        }

        vm.startPrank(owner);
        uniV4 = new UniV4Inspector();
        gate = new PoolGate(address(uniV4));
        vm.stopPrank();

        (bool succ, address addr,) = deployHook(
            abi.encodePacked(type(ExtAngstrom).creationCode, abi.encode(address(uniV4), gov)),
            _angstromFlags(),
            _newFactory()
        );
        assertTrue(succ, "Failed to deploy angstrom");
        angstrom = ExtAngstrom(addr);

        vm.warp(REAL_TIMESTAMP);

        address[] memory nodes = new address[](1);
        nodes[0] = node;
        vm.prank(gov);
        angstrom.govToggleNodes(nodes);
    }

    mapping(uint256 => bool) usedIndices;
    mapping(address trader => mapping(bool isInternal => mapping(address asset => uint256))) traderTotalIn;

    mapping(address => uint256) totalOuts;

    struct test_gas1Vars {
        Trader[] traders;
        Bundle b;
        uint256[] pairOrderCounts;
        uint256[] orderIndexOffset;
        OrderMeta empty;
        TypedDataHasher hasher;
        AmountA totalAOut;
        AmountB totalBOut;
        bool aToB;
        PriceOutVsIn priceOutVsIn;
        PriceOutVsIn minPrice;
        AmountOut amountOut;
        AmountIn amountIn;
        address assetIn;
        address assetOut;
        bool isFlash;
        Trader trader;
        uint256 p;
        uint128 minAmountIn;
        uint128 maxAmountIn;
        uint256 deadline;
    }

    function test_gas_execute_limit_1() public {
        angstrom.updateLastBlock();
        vm.roll(block.number + 1);

        test_gas1Vars memory v;
        PRNG memory rng = _rng("exec_1");

        // Initialize random pairs with order counts.
        v.b.pairs = new Pair[](TOTAL_PAIRS);
        {
            PriceAB[] memory prices = _randPrices(rng, TOTAL_PAIRS);
            v.pairOrderCounts = _randOrderCounts(rng, TOTAL_PAIRS, 12, 32);
            uint256 pairIndex = 0;
            for (uint256 i = 0; i < TOTAL_ASSETS; i++) {
                for (uint256 j = i + 1; j < TOTAL_ASSETS; j++) {
                    Pair memory pair = v.b.pairs[pairIndex];
                    pair.priceAB = prices[pairIndex];
                    pair.assetA = assets[i];
                    pair.assetB = assets[j];
                    pairIndex++;
                    pair._checkOrdered();
                }
            }
            uint256 cumulativeOffset = 0;
            v.orderIndexOffset = new uint256[](TOTAL_PAIRS);
            for (uint256 i = 0; i < TOTAL_PAIRS; i++) {
                v.orderIndexOffset[i] = cumulativeOffset;
                cumulativeOffset += v.pairOrderCounts[i];
            }
            v.b.userOrders = new UserOrder[](v.pairOrderCounts.sum());
        }

        // Initialize trader actors.
        v.traders = makeTraders(TOTAL_ACTORS);

        for (uint256 i = 0; i < v.traders.length; i++) {
            Trader memory trader = v.traders[i];
            vm.prank(trader.addr);
            angstrom.invalidateNonce(u64(trader.getFreshNonce()));
            for (uint256 j = 0; j < assets.length; j++) {
                address asset = assets[j];
                angstrom.__ilegalMint(trader.addr, asset, 1);
                MockERC20(asset).mint(trader.addr, 1);
            }
        }

        v.hasher = TypedDataHasherLib.init(angstrom.DOMAIN_SEPARATOR());

        for (uint256 j = 0; j < TOTAL_PAIRS; j++) {
            Pair memory pair = v.b.pairs[j];

            v.totalAOut = AmountA.wrap(0);
            v.totalBOut = AmountB.wrap(0);

            for (uint256 i = 0; i < v.pairOrderCounts[j]; i++) {
                uint256 oi = v.orderIndexOffset[j] + i;
                if (DEBUG_LOGS) console.log("[%s]", oi);
                assertFalse(usedIndices[oi]);
                usedIndices[oi] = true;

                bool isLast = i == v.pairOrderCounts[j] - 1;
                v.aToB = isLast ? v.totalBOut < pair.priceAB.convert(v.totalAOut) : rng.randbool();

                v.assetOut = v.aToB ? pair.assetB : pair.assetA;
                v.assetIn = v.aToB ? pair.assetA : pair.assetB;
                v.priceOutVsIn = PriceOutVsIn.wrap(v.aToB ? pair.priceAB.into().invRay() : pair.priceAB.into());

                v.amountOut = AmountOut.wrap(
                    isLast
                        ? (
                            v.aToB
                                ? (pair.priceAB.convert(v.totalAOut) - v.totalBOut).into()
                                : (pair.priceAB.convert(v.totalBOut) - v.totalAOut).into()
                        )
                        : rng.randmag((v.priceOutVsIn.into() / 10).rayToWad(), (v.priceOutVsIn.into() * 10).rayToWad())
                );
                v.amountIn = v.priceOutVsIn.convert(v.amountOut);
                if (DEBUG_LOGS) {
                    console.log("  amountIn: %18e", AmountIn.unwrap(v.amountIn));
                    console.log("  amountOut: %18e", AmountOut.unwrap(v.amountOut));
                    console.log("  price: %27e", PriceOutVsIn.unwrap(pair.priceAB));
                }

                if (v.aToB) {
                    v.totalBOut = v.totalBOut + AmountB.wrap(v.amountOut.into());
                } else {
                    v.totalAOut = v.totalAOut + AmountA.wrap(v.amountOut.into());
                }

                v.isFlash = rng.randbool();
                v.minPrice = v.priceOutVsIn.mulRayScalar(rng.randuint(0.89e18, 1.0e18));
                if (DEBUG_LOGS) console.log("v.minPrice: %s", PriceOutVsIn.unwrap(v.minPrice));
                v.trader = v.traders[rng.randuint(v.traders.length)];
                v.deadline = block.timestamp + 240 + rng.randuint(0, 3600);
                bool useInternal;

                v.p = rng.randuint(1e18);
                if (v.p <= 0.4e18) {
                    v.minAmountIn = v.amountIn.into().mulWad(rng.randuint(0.2e18, 1.0e18)).toUint128();
                    v.maxAmountIn = v.amountIn.into().mulWad(rng.randuint(1.0e18, 10.0e18)).toUint128();
                    useInternal = rng.randbool();
                    // Partial order
                    v.b.userOrders[oi] = v.isFlash
                        ? UserOrderLib.from(
                            PartialFlashOrder({
                                minAmountIn: v.minAmountIn.toUint128(),
                                maxAmountIn: v.maxAmountIn.toUint128(),
                                minPrice: v.minPrice.into(),
                                useInternal: useInternal,
                                assetIn: v.assetIn,
                                assetOut: v.assetOut,
                                recipient: v.trader.addr,
                                hook: address(0),
                                hookPayload: new bytes(0),
                                validForBlock: u64(block.number),
                                amountFilled: v.amountIn.into().toUint128(),
                                meta: v.empty
                            })
                        )
                        : UserOrderLib.from(
                            PartialStandingOrder({
                                minAmountIn: v.minAmountIn,
                                maxAmountIn: v.maxAmountIn,
                                minPrice: v.minPrice.into(),
                                useInternal: useInternal,
                                assetIn: v.assetIn,
                                assetOut: v.assetOut,
                                recipient: v.trader.addr,
                                hook: address(0),
                                hookPayload: new bytes(0),
                                nonce: u64(v.trader.getFreshNonce()),
                                deadline: u40(v.deadline),
                                amountFilled: v.amountIn.into().toUint128(),
                                meta: v.empty
                            })
                        );
                } else {
                    bool exactIn = v.p <= 0.7e18;
                    useInternal = rng.randbool();
                    // Exact in/out order
                    v.b.userOrders[oi] = v.isFlash
                        ? UserOrderLib.from(
                            ExactFlashOrder({
                                exactIn: exactIn,
                                amount: (exactIn ? v.amountIn.into() : v.amountOut.into()).toUint128(),
                                minPrice: v.minPrice.into(),
                                useInternal: useInternal,
                                assetIn: v.assetIn,
                                assetOut: v.assetOut,
                                recipient: v.trader.addr,
                                hook: address(0),
                                hookPayload: new bytes(0),
                                validForBlock: u64(block.number),
                                meta: v.empty
                            })
                        )
                        : UserOrderLib.from(
                            ExactStandingOrder({
                                exactIn: exactIn,
                                amount: (exactIn ? v.amountIn.into() : v.amountOut.into()).toUint128(),
                                minPrice: v.minPrice.into(),
                                useInternal: useInternal,
                                assetIn: v.assetIn,
                                assetOut: v.assetOut,
                                recipient: v.trader.addr,
                                hook: address(0),
                                hookPayload: new bytes(0),
                                nonce: u64(v.trader.getFreshNonce()),
                                deadline: u40(v.deadline),
                                meta: v.empty
                            })
                        );
                }

                traderTotalIn[v.trader.addr][useInternal][v.assetIn] += v.amountIn.into();
                v.trader.sign(v.b.userOrders[oi], v.hasher);
            }
            totalOuts[pair.assetA] += v.totalAOut.into();
            totalOuts[pair.assetB] += v.totalBOut.into();
        }

        for (uint256 i = 0; i < v.traders.length; i++) {
            address trader = v.traders[i].addr;
            vm.startPrank(trader);
            for (uint256 j = 0; j < assets.length; j++) {
                address asset = assets[j];
                MockERC20 erc20 = MockERC20(asset);
                {
                    uint256 internalTotal = traderTotalIn[trader][true][asset];
                    angstrom.__ilegalMint(trader, asset, internalTotal);
                    erc20.mint(address(angstrom), internalTotal);
                }
                {
                    erc20.approve(address(angstrom), type(uint256).max);
                    uint256 externalTotal = traderTotalIn[trader][false][asset];
                    erc20.mint(trader, externalTotal);
                }
            }
            vm.stopPrank();
        }

        v.b.assets = new Asset[](assets.length);
        for (uint256 i = 0; i < assets.length; i++) {
            address addr = assets[i];
            Asset memory asset = v.b.assets[i];
            asset.addr = addr;
            asset.borrow = totalOuts[addr].toUint128();
            asset.save = 0;
            asset.settle = totalOuts[addr].toUint128();
            gate.mint(addr, totalOuts[addr]);
        }

        v.b.assets.sort();
        v.b.pairs.sort();

        bytes memory payload = v.b.encode();

        uint256 zeros;
        uint256 nonZeros;
        for (uint256 i = 0; i < payload.length; i++) {
            payload[i] == 0x00 ? zeros++ : nonZeros++;
        }

        console.log("total orders: %s\n", v.b.userOrders.length);
        uint256 cdCost = zeros * 4 + nonZeros * 16;
        console.log("rng.__state: %x", rng.__state);

        uint256 cost = this.__doExecute(payload);
        console.log("execution cost: %s", cost);
        console.log("total: %s", cost + cdCost);
    }

    /// @dev Isolate execution in its own call-context to avoid confounding gas cost factors like
    /// memory allocation.
    function __doExecute(bytes calldata payload) external returns (uint256 cost) {
        vm.prank(node);
        bytes memory execPayload = abi.encodeCall(angstrom.execute, PadeEncoded(payload));
        address angstromAddr = address(angstrom);
        uint256 before = gasleft();
        assembly {
            let success := call(gas(), angstromAddr, 0, add(execPayload, 0x20), mload(execPayload), 0, 0)
            if iszero(success) {
                let free := mload(0x40)
                returndatacopy(free, 0, returndatasize())
                revert(free, returndatasize())
            }
        }
        unchecked {
            cost = before - gasleft();
        }
    }

    function _rng(bytes32 seed) internal pure returns (PRNG memory) {
        return PRNG(uint256(seed));
    }

    function _randPrices(PRNG memory rng, uint256 n) internal pure returns (PriceAB[] memory prices) {
        prices = new PriceAB[](n);
        for (uint256 i = 0; i < n; i++) {
            prices[i] = PriceAB.wrap(rng.randmag(0.01e27, 100.0e27));
        }
    }

    function _randOrderCounts(PRNG memory rng, uint256 n, uint256 low, uint256 high)
        internal
        pure
        returns (uint256[] memory counts)
    {
        counts = new uint256[](n);
        for (uint256 i = 0; i < n; i++) {
            counts[i] = rng.randchoice(0.2e18, 0, rng.randuint(low, high + 1));
        }
    }
}
