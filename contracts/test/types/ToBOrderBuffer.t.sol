// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {Test} from "forge-std/Test.sol";
import {ToBOrderBuffer, ToBOrderBufferLib} from "src/types/ToBOrderBuffer.sol";
import {TopOfBlockOrder} from "test/_reference/OrderTypes.sol";

/// @author philogy <https://github.com/philogy>
contract ToBOrderBufferTest is Test {
    struct Marker {
        uint256 value;
    }

    function test_bufferBytes() public pure {
        ToBOrderBuffer memory buffer;
        Marker memory end = Marker(1);

        uint256 gap;
        assembly ("memory-safe") {
            gap := sub(end, buffer)
        }

        assertEq(gap, ToBOrderBufferLib.BUFFER_BYTES);
    }

    struct ToBERC712 {
        uint128 quantityIn;
        uint128 quantityOut;
        uint128 maxGasAsset0;
        bool useInternal;
        address assetIn;
        address assetOut;
        address recipient;
        uint64 validForBlock;
    }

    function test_fuzzing_bufferTypeHashEqualReference(ToBERC712 calldata order) public {
        TopOfBlockOrder memory refOrder;
        refOrder.quantityIn = order.quantityIn;
        refOrder.quantityOut = order.quantityOut;
        refOrder.maxGasAsset0 = order.maxGasAsset0;
        refOrder.useInternal = order.useInternal;
        refOrder.assetIn = order.assetIn;
        refOrder.assetOut = order.assetOut;
        refOrder.recipient = order.recipient;
        refOrder.validForBlock = order.validForBlock;

        vm.roll(order.validForBlock);

        ToBOrderBuffer memory buffer;
        buffer.init();
        buffer.quantityIn = order.quantityIn;
        buffer.quantityOut = order.quantityOut;
        buffer.maxGasAsset0 = order.maxGasAsset0;
        buffer.useInternal = order.useInternal;
        buffer.assetIn = order.assetIn;
        buffer.assetOut = order.assetOut;
        buffer.recipient = order.recipient;

        assertEq(buffer.hash(), refOrder.hash());
    }

    function _markMem() internal pure {
        bytes32 marker = (keccak256("marker"));
        assembly ("memory-safe") {
            let free := mload(0x40)
            mstore(0x40, add(free, 0x20))
            mstore(free, marker)
        }
    }
}
