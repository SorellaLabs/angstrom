// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "test/_helpers/BaseTest.sol";
import {UserOrderBuffer} from "src/types/UserOrderBuffer.sol";
import {
    PartialStandingOrder,
    ExactStandingOrder,
    PartialFlashOrder,
    ExactFlashOrder
} from "test/_reference/OrderTypes.sol";
import {UserOrderVariantMap} from "src/types/UserOrderVariantMap.sol";
import {OrderVariant} from "test/_reference/OrderVariant.sol";
import {CalldataReader, CalldataReaderLib} from "src/types/CalldataReader.sol";

import {console} from "forge-std/console.sol";

/// @author philogy <https://github.com/philogy>
contract UserOrderBufferTest is BaseTest {
    function setUp() public {}

    function test_fuzzing_referenceEqBuffer_PartialStandingOrder(PartialStandingOrder memory order)
        public
        view
    {
        OrderVariant memory varIn;
        varIn.isExact = false;
        varIn.isFlash = false;
        varIn.isOut = false;
        varIn.noHook = order.hook == address(0);
        varIn.useInternal = order.useInternal;
        varIn.hasRecipient = order.recipient != address(0);

        UserOrderVariantMap varMap = varIn.encode();

        this._test_fuzzing_referenceEqBuffer_PartialStandingOrder(
            order,
            bytes.concat(
                bytes1(UserOrderVariantMap.unwrap(varMap)),
                bytes4(order.refId),
                bytes8(order.nonce),
                bytes5(order.deadline)
            )
        );
    }

    function _test_fuzzing_referenceEqBuffer_PartialStandingOrder(
        PartialStandingOrder memory order,
        bytes calldata dataStart
    ) external view {
        CalldataReader reader = CalldataReaderLib.from(dataStart);
        UserOrderBuffer memory buffer;
        UserOrderVariantMap varMap;
        (reader, varMap) = buffer.init(reader);

        buffer.exactIn_or_minQuantityIn = order.minAmountIn;
        buffer.quantity_or_maxQuantityIn = order.maxAmountIn;
        buffer.maxGasAsset0 = order.maxGasAsset0;
        buffer.minPrice = order.minPrice;
        buffer.useInternal = order.useInternal;
        buffer.assetIn = order.assetIn;
        buffer.assetOut = order.assetOut;
        buffer.recipient = order.recipient;
        buffer.hookDataHash = keccak256(
            order.hook == address(0)
                ? new bytes(0)
                : bytes.concat(bytes20(order.hook), order.hookPayload)
        );
        buffer.readOrderValidation(reader, varMap);

        assertEq(buffer._hash(varMap), order.hash());
    }

    function test_fuzzing_referenceEqBuffer_ExactStandingOrder(ExactStandingOrder memory order)
        public
        view
    {
        OrderVariant memory varIn;
        varIn.isExact = true;
        varIn.isFlash = false;
        varIn.isOut = !order.exactIn;
        varIn.noHook = order.hook == address(0);
        varIn.useInternal = order.useInternal;
        varIn.hasRecipient = order.recipient != address(0);

        UserOrderVariantMap varMap = varIn.encode();

        this._test_fuzzing_referenceEqBuffer_ExactStandingOrder(
            order,
            bytes.concat(
                bytes1(UserOrderVariantMap.unwrap(varMap)),
                bytes4(order.refId),
                bytes8(order.nonce),
                bytes5(order.deadline)
            )
        );
    }

    function _test_fuzzing_referenceEqBuffer_ExactStandingOrder(
        ExactStandingOrder memory order,
        bytes calldata dataStart
    ) external view {
        CalldataReader reader = CalldataReaderLib.from(dataStart);
        UserOrderBuffer memory buffer;
        UserOrderVariantMap varMap;
        (reader, varMap) = buffer.init(reader);

        buffer.exactIn_or_minQuantityIn = order.exactIn ? 1 : 0;
        buffer.quantity_or_maxQuantityIn = order.amount;
        buffer.maxGasAsset0 = order.maxGasAsset0;
        buffer.minPrice = order.minPrice;
        buffer.useInternal = order.useInternal;
        buffer.assetIn = order.assetIn;
        buffer.assetOut = order.assetOut;
        buffer.recipient = order.recipient;
        buffer.hookDataHash = keccak256(
            order.hook == address(0)
                ? new bytes(0)
                : bytes.concat(bytes20(order.hook), order.hookPayload)
        );
        buffer.readOrderValidation(reader, varMap);

        assertEq(buffer._hash(varMap), order.hash());
    }

    function test_fuzzing_referenceEqBuffer_PartialFlashOrder(PartialFlashOrder memory order)
        public
    {
        OrderVariant memory varIn;
        varIn.isExact = false;
        varIn.isFlash = true;
        varIn.isOut = false;
        varIn.noHook = order.hook == address(0);
        varIn.useInternal = order.useInternal;
        varIn.hasRecipient = order.recipient != address(0);

        UserOrderVariantMap varMap = varIn.encode();

        vm.roll(order.validForBlock);
        this._test_fuzzing_referenceEqBuffer_PartialFlashOrder(
            order, bytes.concat(bytes1(UserOrderVariantMap.unwrap(varMap)), bytes4(order.refId))
        );
    }

    function _test_fuzzing_referenceEqBuffer_PartialFlashOrder(
        PartialFlashOrder memory order,
        bytes calldata dataStart
    ) external view {
        CalldataReader reader = CalldataReaderLib.from(dataStart);
        UserOrderBuffer memory buffer;
        UserOrderVariantMap varMap;
        (reader, varMap) = buffer.init(reader);

        buffer.exactIn_or_minQuantityIn = order.minAmountIn;
        buffer.quantity_or_maxQuantityIn = order.maxAmountIn;
        buffer.maxGasAsset0 = order.maxGasAsset0;
        buffer.minPrice = order.minPrice;
        buffer.useInternal = order.useInternal;
        buffer.assetIn = order.assetIn;
        buffer.assetOut = order.assetOut;
        buffer.recipient = order.recipient;
        buffer.hookDataHash = keccak256(
            order.hook == address(0)
                ? new bytes(0)
                : bytes.concat(bytes20(order.hook), order.hookPayload)
        );
        buffer.readOrderValidation(reader, varMap);

        assertEq(buffer._hash(varMap), order.hash());
    }

    function test_fuzzing_referenceEqBuffer_ExactFlashOrder(ExactFlashOrder memory order) public {
        OrderVariant memory varIn;
        varIn.isExact = true;
        varIn.isFlash = true;
        varIn.isOut = !order.exactIn;
        varIn.noHook = order.hook == address(0);
        varIn.useInternal = order.useInternal;
        varIn.hasRecipient = order.recipient != address(0);

        UserOrderVariantMap varMap = varIn.encode();

        vm.roll(order.validForBlock);
        this._test_fuzzing_referenceEqBuffer_ExactFlashOrder(
            order, bytes.concat(bytes1(UserOrderVariantMap.unwrap(varMap)), bytes4(order.refId))
        );
    }

    function _test_fuzzing_referenceEqBuffer_ExactFlashOrder(
        ExactFlashOrder memory order,
        bytes calldata dataStart
    ) external view {
        CalldataReader reader = CalldataReaderLib.from(dataStart);
        UserOrderBuffer memory buffer;
        UserOrderVariantMap varMap;
        (reader, varMap) = buffer.init(reader);

        buffer.exactIn_or_minQuantityIn = order.exactIn ? 1 : 0;
        buffer.quantity_or_maxQuantityIn = order.amount;
        buffer.maxGasAsset0 = order.maxGasAsset0;
        buffer.minPrice = order.minPrice;
        buffer.useInternal = order.useInternal;
        buffer.assetIn = order.assetIn;
        buffer.assetOut = order.assetOut;
        buffer.recipient = order.recipient;
        buffer.hookDataHash = keccak256(
            order.hook == address(0)
                ? new bytes(0)
                : bytes.concat(bytes20(order.hook), order.hookPayload)
        );
        buffer.readOrderValidation(reader, varMap);

        assertEq(buffer._hash(varMap), order.hash());
    }

    function test_ffi_stuff() public {
        string[] memory args = new string[](5);
        args[0] = "test/_reference/eip712.py";
        args[1] = "test/_reference/OrderTypes.sol:OrderMeta";
        args[2] = "false";
        args[3] = "0x33CC24dbf9c8FDDB574077eE0Fa1d2b93B566381";
        args[4] = "0x01";
        bytes memory res = ffiPython(args);
        console.logBytes(res);
    }
}
