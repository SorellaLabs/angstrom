// SPDX-License-Identifier: BUSL-1.1
pragma solidity 0.8.24;

import {ERC712} from "./base/ERC712.sol";
import {NodeManager} from "./base/NodeManager.sol";
import {Accounter} from "./base/Accounter.sol";
import {UnorderedNonces} from "./libraries/UnorderedNonces.sol";

import {Globals} from "./libraries/Globals.sol";
import {tuint256} from "transient-goodies/TransientPrimitives.sol";
import {PriceGraphLib, PriceGraph, AssetIndex} from "./libraries/PriceGraph.sol";
import {GenericOrder, TopOfBlockOrderEnvelope, OrderType, OrderMode, AssetForm} from "./interfaces/OrderTypes.sol";

import {SafeCastLib} from "solady/src/utils/SafeCastLib.sol";
import {FixedPointMathLib} from "solady/src/utils/FixedPointMathLib.sol";
import {SignatureCheckerLib} from "solady/src/utils/SignatureCheckerLib.sol";
import {RayMathLib} from "./libraries/RayMathLib.sol";

import {IAngstromComposable} from "./interfaces/IAngstromComposable.sol";
import {IUnlockCallback} from "v4-core/src/interfaces/callback/IUnlockCallback.sol";
import {IPoolManager, IUniV4} from "./interfaces/IUniV4.sol";

// TODO: Remove debug helpers
import {console2 as console} from "forge-std/console2.sol";

/// @author philogy <https://github.com/philogy>
contract Angstrom is Accounter, ERC712, UnorderedNonces, NodeManager, IUnlockCallback {
    using RayMathLib for uint256;
    using IUniV4 for IPoolManager;
    using SafeCastLib for uint256;
    using FixedPointMathLib for uint256;

    error AssetsOutOfOrder();
    error OnlyOncePerBlock();

    error LimitViolated();
    error Expired();
    error InvalidHookReturn();
    error OrderAlreadyExecuted();

    error FillingTooMuch();
    error InvalidSignature();
    error Unresolved();

    // persistent storage
    uint256 public lastBlockUpdated;
    uint256 public halfSpreadRay;

    // transient storage
    mapping(bytes32 => tuint256) internal alreadyExecuted;

    struct Price {
        AssetIndex outIndex;
        AssetIndex inIndex;
        uint256 price;
    }
    // RAY 1e27

    constructor(address uniV4PoolManager, address governance) Accounter(uniV4PoolManager) NodeManager(governance) {}

    function execute(bytes calldata data) external onlyNode {
        UNI_V4.unlock(data);
    }

    function unlockCallback(bytes calldata data) external override onlyUniV4 returns (bytes memory) {
        // TODO: Optimize, letting solc do this is terribly inefficient.
        (
            address[] memory assets,
            Price[] memory initialPrices,
            bytes[] memory preTransformations,
            TopOfBlockOrderEnvelope[] memory topOfBlockOrders,
            IUniV4.Swap[] memory swaps,
            GenericOrder[] memory orders,
            bytes[] memory postTransformations,
            Donate[] memory donates
        ) = abi.decode(
            data,
            (address[], Price[], bytes[], TopOfBlockOrderEnvelope[], IUniV4.Swap[], GenericOrder[], bytes[], Donate[])
        );

        Globals memory g = _validateAndInitGlobals(assets, initialPrices);

        _dispatchTransformations(g, preTransformations);

        _validateAndExecuteToB(g, topOfBlockOrders);

        _validateAndExecuteOrders(g, orders);

        _dispatchTransformations(g, postTransformations);

        // Execute swaps.
        for (uint256 i = 0; i < swaps.length; i++) {
            UNI_V4.swap(swaps[i], g);
        }

        for (uint256 i = 0; i < donates.length; i++) {
            _donate(g, donates[i]);
        }

        _validateAndResolveFinal();

        return new bytes(0);
    }

    function _validateAndInitGlobals(address[] memory assets, Price[] memory initialPrices)
        internal
        returns (Globals memory)
    {
        // Global bundle lock (prevents reentrancy & replaying flash orders).
        if (lastBlockUpdated == block.number) revert OnlyOncePerBlock();
        lastBlockUpdated = block.number;

        // Validate asset list.
        address lastAsset = assets[0];
        for (uint256 i = 1; i < assets.length; i++) {
            address nextAsset = assets[i];
            if (nextAsset <= lastAsset) revert AssetsOutOfOrder();
            lastAsset = nextAsset;
        }

        // Initialize and validate price graph.
        PriceGraph prices = PriceGraphLib.init(assets.length);
        for (uint256 i = 0; i < initialPrices.length; i++) {
            Price memory init = initialPrices[i];
            prices.set(init.outIndex, init.inIndex, init.price);
        }

        return Globals({prices: prices, assets: assets});
    }

    function _dispatchTransformations(Globals memory, bytes[] memory transformations) internal {
        for (uint256 i = 0; i < transformations.length; i++) {
            (bool success,) = address(this).call(transformations[i]);
            require(success);
        }
    }

    function _validateAndExecuteToB(Globals memory g, TopOfBlockOrderEnvelope[] memory orders) internal {
        for (uint256 i = 0; i < orders.length; i++) {
            TopOfBlockOrderEnvelope memory order = orders[i];

            address assetIn = g.get(order.assetInIndex);
            address assetOut = g.get(order.assetOutIndex);

            // The `.hash` method validates the `block.number` for flash orders.
            bytes32 orderHash = order.hash(assetIn, assetOut);

            tuint256 storage executed = alreadyExecuted[orderHash];
            if (executed.get() != 0) revert OrderAlreadyExecuted();
            executed.set(1);

            if (!SignatureCheckerLib.isValidSignatureNow(order.from, _hashTypedData(orderHash), order.signature)) {
                revert InvalidSignature();
            }

            if (order.hook != address(0)) {
                if (
                    IAngstromComposable(order.hook).compose(order.from, order.hookPayload)
                        != ~uint32(IAngstromComposable.compose.selector)
                ) revert InvalidHookReturn();
            }

            _accountIn(order.from, order.assetInForm, assetIn, order.amountIn);
            _accountOut(order.from, order.assetOutForm, assetOut, order.amountOut);
        }
    }

    function _validateAndExecuteOrders(Globals memory g, GenericOrder[] memory orders) internal {
        for (uint256 i = 0; i < orders.length; i++) {
            GenericOrder memory order = orders[i];
            uint256 price = g.prices.get(order.assetOutIndex, order.assetInIndex);
            if (price < order.minPrice) revert LimitViolated();

            address assetIn = g.get(order.assetInIndex);
            address assetOut = g.get(order.assetOutIndex);
            // The `.hash` method validates the `block.number` for flash orders.
            bytes32 orderHash = order.hash(assetIn, assetOut);

            if (!SignatureCheckerLib.isValidSignatureNow(order.from, _hashTypedData(orderHash), order.signature)) {
                revert InvalidSignature();
            }

            if (order.otype == OrderType.Standing) {
                if (block.timestamp > order.deadline) revert Expired();
                _useNonce(order.from, order.nonce);
            } else {
                tuint256 storage executed = alreadyExecuted[orderHash];
                if (executed.get() != 0) revert OrderAlreadyExecuted();
                executed.set(1);
            }

            if (order.hook != address(0)) {
                if (
                    IAngstromComposable(order.hook).compose(order.from, order.hookPayload)
                        != ~uint32(IAngstromComposable.compose.selector)
                ) revert InvalidHookReturn();
            }

            (uint256 amountIn, uint256 amountOut) = _getAmounts(order, price);
            _accountIn(order.from, order.assetInForm, assetIn, amountIn);
            _accountOut(order.from, order.assetOutForm, assetOut, amountOut);
        }
    }

    function _getAmounts(GenericOrder memory order, uint256 price)
        internal
        view
        returns (uint256 amountIn, uint256 amountOut)
    {
        uint256 feeRay = halfSpreadRay;
        if (order.mode == OrderMode.ExactIn) {
            amountIn = order.amountSpecified;
            amountOut = amountIn.rayDiv(price);
            amountOut -= amountOut.rayMul(feeRay);
        } else if (order.mode == OrderMode.ExactOut) {
            amountOut = order.amountSpecified;
            amountIn = amountOut.rayMul(price);
            amountIn += amountIn.rayMul(feeRay);
        } else if (order.mode == OrderMode.Partial) {
            amountIn = order.amountFilled;
            if (amountIn > order.amountSpecified) revert FillingTooMuch();
            amountOut = amountIn.rayDiv(price);
            amountOut -= amountOut.rayMul(feeRay);
        } else {
            assert(false);
        }
    }

    function _validateAndResolveFinal() internal view {
        if (unresolvedChanges.get() != 0) revert Unresolved();
    }
}
