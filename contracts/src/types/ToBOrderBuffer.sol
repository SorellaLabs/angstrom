// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import {OrdersLib} from "../reference/OrderTypes.sol";
import {AmountA as AmountOut, AmountB as AmountIn} from "./Price.sol";

import {console} from "forge-std/console.sol";
import {FormatLib} from "super-sol/libraries/FormatLib.sol";

struct ToBOrderBuffer {
    bytes32 typeHash;
    uint256 quantityIn;
    uint256 quantityOut;
    uint256 maxGasAsset0;
    bool useInternal;
    address assetIn;
    address assetOut;
    address recipient;
    uint64 validForBlock;
}

using ToBOrderBufferLib for ToBOrderBuffer global;

/// @author philogy <https://github.com/philogy>
library ToBOrderBufferLib {
    using FormatLib for *;

    uint256 internal constant BUFFER_BYTES = 288;

    function setTypeHash(ToBOrderBuffer memory self) internal pure {
        self.typeHash = OrdersLib.TOP_OF_BLOCK_ORDER_TYPEHASH;
    }

    function hash(ToBOrderBuffer memory self) internal view returns (bytes32 orderHash) {
        self.validForBlock = uint64(block.number);
        assembly ("memory-safe") {
            orderHash := keccak256(self, BUFFER_BYTES)
        }
    }
}
