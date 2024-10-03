// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import {AmountA as AmountOut, AmountB as AmountIn} from "./Price.sol";

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
    uint256 internal constant BUFFER_BYTES = 288;

    /// forgefmt: disable-next-item
    bytes32 internal constant TOP_OF_BLOCK_ORDER_TYPEHASH = keccak256(
        "TopOfBlockOrder("
           "uint128 quantity_in,"
           "uint128 quantity_out,"
           "uint128 max_gas_asset0,"
           "bool use_internal,"
           "address asset_in,"
           "address asset_out,"
           "address recipient,"
           "uint256 valid_for_block"
        ")"
    );

    function init(ToBOrderBuffer memory self) internal pure {
        self.typeHash = TOP_OF_BLOCK_ORDER_TYPEHASH;
    }

    function hash(ToBOrderBuffer memory self) internal view returns (bytes32 orderHash) {
        self.validForBlock = uint64(block.number);
        assembly ("memory-safe") {
            orderHash := keccak256(self, BUFFER_BYTES)
        }
    }
}
