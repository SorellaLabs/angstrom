// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {BaseTest} from "test/_helpers/BaseTest.sol";
import {TWAPOrderBuffer, TWAPOrderBufferLib} from "src/types/TWAPOrderBuffer.sol";
import {TimeWeightedAveragePriceOrder} from "test/_reference/OrderTypes.sol";
import {TWAPOrderVariantMap} from "src/types/TWAPOrderVariantMap.sol";
import {OrderVariant} from "test/_reference/OrderVariant.sol";
import {CalldataReader, CalldataReaderLib} from "src/types/CalldataReader.sol";

/// @author philogy <https://github.com/philogy>
contract TWAPOrderBufferTest is BaseTest {
    function setUp() public {}

    function test_fuzzing_referenceEqBuffer_TWAPOrder(TimeWeightedAveragePriceOrder memory order)
        public
        view
    {
        assertEq(bufferHash(order), order.hash());
    }

    function test_ffi_fuzzing_bufferPythonEquivalence_TWAPOrder(
        TimeWeightedAveragePriceOrder memory order
    ) public {
        assertEq(bufferHash(order), ffiPythonEIP712Hash(order));
    }

    function bufferHash(TimeWeightedAveragePriceOrder memory order)
        internal
        view
        returns (bytes32)
    {
        return this._bufferHashTWAPOrder(
            order,
            bytes.concat(
                bytes1(order.toVariantMap(false)),
                bytes4(order.refId),
                bytes8(order.nonce),
                bytes5(order.startTime),
                bytes4(order.totalParts),
                bytes4(order.timeInterval),
                bytes4(order.window)
            )
        );
    }

    function _bufferHashTWAPOrder(
        TimeWeightedAveragePriceOrder memory order,
        bytes calldata dataStart
    ) external pure returns (bytes32) {
        CalldataReader reader = CalldataReaderLib.from(dataStart);
        TWAPOrderBuffer memory buffer;
        TWAPOrderVariantMap varMap;
        buffer.setTypeHash();
        (reader, varMap) = buffer.init(reader);

        buffer.exactIn = order.exactIn;
        buffer.quantity = order.amount;
        buffer.maxExtraFeeAsset0 = order.maxExtraFeeAsset0;
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
        buffer.readTWAPOrderValidation(reader);
        return buffer.hash();
    }

    function ffiPythonEIP712Hash(TimeWeightedAveragePriceOrder memory order)
        internal
        returns (bytes32)
    {
        string[] memory args = new string[](17);
        args[0] = "test/_reference/eip712.py";
        args[1] = "test/_reference/SignedTypes.sol:TimeWeightedAveragePriceOrder";
        uint256 i = 2;
        args[i++] = vm.toString(order.refId);
        args[i++] = vm.toString(order.exactIn);
        args[i++] = vm.toString(order.amount);
        args[i++] = vm.toString(order.maxExtraFeeAsset0);
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
        args[i++] = vm.toString(order.startTime);
        args[i++] = vm.toString(order.totalParts);
        args[i++] = vm.toString(order.timeInterval);
        args[i++] = vm.toString(order.window);
        return bytes32(ffiPython(args));
    }
}
