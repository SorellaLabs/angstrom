// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {Test} from "forge-std/Test.sol";
import {ToBOrderBuffer, ToBOrderBufferLib} from "src/types/ToBOrderBuffer.sol";
import {TopOfBlockOrder} from "src/reference/OrderTypes.sol";

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
        bool useInternal;
        address assetIn;
        address assetOut;
        address recipient;
        address hook;
        bytes hookPayload;
        uint64 validForBlock;
    }

    function test_fuzzing_bufferTypeHashEqualReference(ToBERC712 calldata order) public {
        TopOfBlockOrder memory refOrder;
        refOrder.quantityIn = order.quantityIn;
        refOrder.quantityOut = order.quantityOut;
        refOrder.useInternal = order.useInternal;
        refOrder.assetIn = order.assetIn;
        refOrder.assetOut = order.assetOut;
        refOrder.recipient = order.recipient;
        refOrder.hook = order.hook;
        refOrder.hookPayload = order.hookPayload;
        refOrder.validForBlock = order.validForBlock;

        vm.roll(order.validForBlock);

        ToBOrderBuffer memory buffer;
        buffer.setTypeHash();
        buffer.quantityIn = order.quantityIn;
        buffer.quantityOut = order.quantityOut;
        buffer.useInternal = order.useInternal;
        buffer.assetIn = order.assetIn;
        buffer.assetOut = order.assetOut;
        buffer.recipient = order.recipient;
        buffer.hookDataHash =
            order.hook == address(0) ? keccak256("") : keccak256(abi.encodePacked(order.hook, order.hookPayload));

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
