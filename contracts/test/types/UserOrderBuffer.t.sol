// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "test/_helpers/BaseTest.sol";
import {UserOrderBuffer, UserOrderBufferLib} from "src/types/UserOrderBuffer.sol";
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
        assertEq(bufferHash(order), order.hash());
    }

    function test_fuzzing_referenceEqBuffer_ExactStandingOrder(ExactStandingOrder memory order)
        public
        view
    {
        assertEq(bufferHash(order), order.hash());
    }

    function test_fuzzing_referenceEqBuffer_PartialFlashOrder(PartialFlashOrder memory order)
        public
    {
        assertEq(bufferHash(order), order.hash());
    }

    function test_fuzzing_referenceEqBuffer_ExactFlashOrder(ExactFlashOrder memory order) public {
        assertEq(bufferHash(order), order.hash());
    }

    function test_ffi_fuzzing_bufferPythonEquivalence_PartialStandingOrder(
        PartialStandingOrder memory order
    ) public {
        assertEq(bufferHash(order), ffiPythonEIP712Hash(order));
    }

    function test_ffi_fuzzing_bufferPythonEquivalence_ExactStandingOrder(
        ExactStandingOrder memory order
    ) public {
        assertEq(bufferHash(order), ffiPythonEIP712Hash(order));
    }

    function test_ffi_fuzzing_bufferPythonEquivalence_PartialFlashOrder(
        PartialFlashOrder memory order
    ) public {
        assertEq(bufferHash(order), ffiPythonEIP712Hash(order));
    }

    function test_ffi_fuzzing_bufferPythonEquivalence_ExactFlashOrder(ExactFlashOrder memory order)
        public
    {
        assertEq(bufferHash(order), ffiPythonEIP712Hash(order));
    }

    function bufferHash(PartialStandingOrder memory order) internal view returns (bytes32) {
        OrderVariant memory varIn;
        varIn.isExact = false;
        varIn.isFlash = false;
        varIn.isOut = false;
        varIn.noHook = order.hook == address(0);
        varIn.useInternal = order.useInternal;
        varIn.hasRecipient = order.recipient != address(0);

        UserOrderVariantMap varMap = varIn.encode();

        return this._bufferHashPartialStandingOrder(
            order,
            bytes.concat(
                bytes1(UserOrderVariantMap.unwrap(varMap)),
                bytes4(order.refId),
                bytes8(order.nonce),
                bytes5(order.deadline)
            )
        );
    }

    function _bufferHashPartialStandingOrder(
        PartialStandingOrder memory order,
        bytes calldata dataStart
    ) external view returns (bytes32) {
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

        return buffer.structHash(varMap);
    }

    function ffiPythonEIP712Hash(PartialStandingOrder memory order) internal returns (bytes32) {
        string[] memory args = new string[](14);
        args[0] = "test/_reference/eip712.py";
        args[1] = "test/_reference/SignedTypes.sol:PartialStandingOrder";
        uint256 i = 2;
        args[i++] = vm.toString(order.refId);
        args[i++] = vm.toString(order.minAmountIn);
        args[i++] = vm.toString(order.maxAmountIn);
        args[i++] = vm.toString(order.maxGasAsset0);
        args[i++] = vm.toString(order.minPrice);
        args[i++] = vm.toString(order.useInternal);
        args[i++] = vm.toString(order.assetIn);
        args[i++] = vm.toString(order.assetOut);
        args[i++] = vm.toString(order.recipient);
        args[i++] = vm.toString(
            order.hook == address(0)
                ? new bytes(0)
                : bytes.concat(bytes20(order.hook), order.hookPayload)
        );
        args[i++] = vm.toString(order.nonce);
        args[i++] = vm.toString(order.deadline);
        return bytes32(ffiPython(args));
    }

    function bufferHash(ExactStandingOrder memory order) internal view returns (bytes32) {
        OrderVariant memory varIn;
        varIn.isExact = true;
        varIn.isFlash = false;
        varIn.isOut = !order.exactIn;
        varIn.noHook = order.hook == address(0);
        varIn.useInternal = order.useInternal;
        varIn.hasRecipient = order.recipient != address(0);

        UserOrderVariantMap varMap = varIn.encode();

        return this._bufferHashExactStandingOrder(
            order,
            bytes.concat(
                bytes1(UserOrderVariantMap.unwrap(varMap)),
                bytes4(order.refId),
                bytes8(order.nonce),
                bytes5(order.deadline)
            )
        );
    }

    function _bufferHashExactStandingOrder(
        ExactStandingOrder memory order,
        bytes calldata dataStart
    ) external view returns (bytes32) {
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

        return buffer.structHash(varMap);
    }

    function ffiPythonEIP712Hash(ExactStandingOrder memory order) internal returns (bytes32) {
        string[] memory args = new string[](14);
        args[0] = "test/_reference/eip712.py";
        args[1] = "test/_reference/SignedTypes.sol:ExactStandingOrder";
        uint256 i = 2;
        args[i++] = vm.toString(order.refId);
        args[i++] = vm.toString(order.exactIn);
        args[i++] = vm.toString(order.amount);
        args[i++] = vm.toString(order.maxGasAsset0);
        args[i++] = vm.toString(order.minPrice);
        args[i++] = vm.toString(order.useInternal);
        args[i++] = vm.toString(order.assetIn);
        args[i++] = vm.toString(order.assetOut);
        args[i++] = vm.toString(order.recipient);
        args[i++] = vm.toString(
            order.hook == address(0)
                ? new bytes(0)
                : bytes.concat(bytes20(order.hook), order.hookPayload)
        );
        args[i++] = vm.toString(order.nonce);
        args[i++] = vm.toString(order.deadline);
        return bytes32(ffiPython(args));
    }

    function bufferHash(PartialFlashOrder memory order) internal returns (bytes32) {
        OrderVariant memory varIn;
        varIn.isExact = false;
        varIn.isFlash = true;
        varIn.isOut = false;
        varIn.noHook = order.hook == address(0);
        varIn.useInternal = order.useInternal;
        varIn.hasRecipient = order.recipient != address(0);

        UserOrderVariantMap varMap = varIn.encode();

        vm.roll(order.validForBlock);
        return this._bufferHashPartialFlashOrder(
            order, bytes.concat(bytes1(UserOrderVariantMap.unwrap(varMap)), bytes4(order.refId))
        );
    }

    function _bufferHashPartialFlashOrder(PartialFlashOrder memory order, bytes calldata dataStart)
        external
        view
        returns (bytes32)
    {
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

        return buffer.structHash(varMap);
    }

    function ffiPythonEIP712Hash(PartialFlashOrder memory order) internal returns (bytes32) {
        string[] memory args = new string[](13);
        args[0] = "test/_reference/eip712.py";
        args[1] = "test/_reference/SignedTypes.sol:PartialFlashOrder";
        uint256 i = 2;
        args[i++] = vm.toString(order.refId);
        args[i++] = vm.toString(order.minAmountIn);
        args[i++] = vm.toString(order.maxAmountIn);
        args[i++] = vm.toString(order.maxGasAsset0);
        args[i++] = vm.toString(order.minPrice);
        args[i++] = vm.toString(order.useInternal);
        args[i++] = vm.toString(order.assetIn);
        args[i++] = vm.toString(order.assetOut);
        args[i++] = vm.toString(order.recipient);
        args[i++] = vm.toString(
            order.hook == address(0)
                ? new bytes(0)
                : bytes.concat(bytes20(order.hook), order.hookPayload)
        );
        args[i++] = vm.toString(order.validForBlock);
        return bytes32(ffiPython(args));
    }

    function bufferHash(ExactFlashOrder memory order) internal returns (bytes32) {
        OrderVariant memory varIn;
        varIn.isExact = true;
        varIn.isFlash = true;
        varIn.isOut = !order.exactIn;
        varIn.noHook = order.hook == address(0);
        varIn.useInternal = order.useInternal;
        varIn.hasRecipient = order.recipient != address(0);

        UserOrderVariantMap varMap = varIn.encode();

        vm.roll(order.validForBlock);
        return this._bufferHashExactFlashOrder(
            order, bytes.concat(bytes1(UserOrderVariantMap.unwrap(varMap)), bytes4(order.refId))
        );
    }

    function _bufferHashExactFlashOrder(ExactFlashOrder memory order, bytes calldata dataStart)
        external
        view
        returns (bytes32)
    {
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

        return buffer.structHash(varMap);
    }

    function ffiPythonEIP712Hash(ExactFlashOrder memory order) internal returns (bytes32) {
        string[] memory args = new string[](13);
        args[0] = "test/_reference/eip712.py";
        args[1] = "test/_reference/SignedTypes.sol:ExactFlashOrder";
        uint256 i = 2;
        args[i++] = vm.toString(order.refId);
        args[i++] = vm.toString(order.exactIn);
        args[i++] = vm.toString(order.amount);
        args[i++] = vm.toString(order.maxGasAsset0);
        args[i++] = vm.toString(order.minPrice);
        args[i++] = vm.toString(order.useInternal);
        args[i++] = vm.toString(order.assetIn);
        args[i++] = vm.toString(order.assetOut);
        args[i++] = vm.toString(order.recipient);
        args[i++] = vm.toString(
            order.hook == address(0)
                ? new bytes(0)
                : bytes.concat(bytes20(order.hook), order.hookPayload)
        );
        args[i++] = vm.toString(order.validForBlock);
        return bytes32(ffiPython(args));
    }
}

/*
buffer:
81bee54f5bc89c9f16c456744a6b7aae57ddc35f1492d343cb7678564c8a6c13
00000000000000000000000000000000000000000000000000000000000015ec
00000000000000000000000000000000000000000000000000000000000037ac
00000000000000000000000000000000000000000000000000000000000014c8
0000000000000000000000000000000000000000000000000000000000000ce8
000000000000000000000000000000000000000000000000000000000000489c
0000000000000000000000000000000000000000000000000000000000000000
0000000000000000000000000000000000000000000000000000000000002ca9
00000000000000000000000000000000000000000000000000000000000002d9
0000000000000000000000000000000000000000000000000000000000003cfb
10f4dece0bf840bc0bb3f486812a1d29a7cb63617c822d63b742daab1d3e3940
0000000000000000000000000000000000000000000000000000000000002d14
0000000000000000000000000000000000000000000000000000000000003f5f

python:
81bee54f5bc89c9f16c456744a6b7aae57ddc35f1492d343cb7678564c8a6c13
00000000000000000000000000000000000000000000000000000000000015ec
00000000000000000000000000000000000000000000000000000000000037ac
00000000000000000000000000000000000000000000000000000000000014c8
0000000000000000000000000000000000000000000000000000000000000ce8
000000000000000000000000000000000000000000000000000000000000489c
0000000000000000000000000000000000000000000000000000000000000001
0000000000000000000000000000000000000000000000000000000000002ca9
00000000000000000000000000000000000000000000000000000000000002d9
0000000000000000000000000000000000000000000000000000000000003cfb
10f4dece0bf840bc0bb3f486812a1d29a7cb63617c822d63b742daab1d3e3940
0000000000000000000000000000000000000000000000000000000000002d14
0000000000000000000000000000000000000000000000000000000000003f5f
*/
