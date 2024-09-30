// SPDX-License-Identifier: BUSL-1.1
pragma solidity ^0.8.0;

import {IPoolManager} from "v4-core/src/interfaces/IPoolManager.sol";
import {PoolId} from "v4-core/src/types/PoolId.sol";
import {POOL_FEE} from "src/Constants.sol";

// forgefmt: disable-next-item
struct UniSwapCallBuffer {
    uint256 leftPaddedSelector;
    /* 0x000 */ address asset0;
    /* 0x020 */ address asset1;
    /* 0x040 */ uint24 fee;
    /* 0x060 */ int24 tickSpacing;
    /* 0x080 */ address hook;
    /* 0x0a0 */ bool zeroForOne;
    /* 0x0c0 */ int256 amountSpecified;
    /* 0x0e0 */ uint160 sqrtPriceLimitX96;
    /* 0x100 */ uint256 hookDataRelativeOffset;
    /* 0x120 */ uint256 hookDataLength;
}

using UniCallLib for UniSwapCallBuffer global;

/// @author philogy <https://github.com/philogy>
library UniCallLib {
    error SwapFailed();

    /// @dev Uniswap's `MIN_SQRT_RATIO + 1` to pass the limit check.
    uint160 internal constant MIN_SQRT_RATIO = 4295128740;
    /// @dev Uniswap's `MAX_SQRT_RATIO - 1` to pass the limit check.
    uint160 internal constant MAX_SQRT_RATIO = 1461446703485210103287273052203988822378723970341;

    uint256 internal constant HOOK_DATA_CD_REL_OFFSET = 0x120;
    uint256 internal constant CALL_PAYLOAD_START_OFFSET = 28;
    uint256 internal constant CALL_PAYLOAD_CD_BYTES = 0x144;
    uint256 internal constant POOL_KEY_OFFSET = 0x20;
    uint256 internal constant POOL_KEY_SIZE = 0xa0;

    function newSwapCall(address hook)
        internal
        pure
        returns (UniSwapCallBuffer memory callBuffer)
    {
        callBuffer.leftPaddedSelector = uint256(uint32(IPoolManager.swap.selector));
        callBuffer.hook = hook;
        callBuffer.hookDataRelativeOffset = HOOK_DATA_CD_REL_OFFSET;
    }

    function setZeroForOne(UniSwapCallBuffer memory self, bool zeroForOne) internal pure {
        self.zeroForOne = zeroForOne;
        self.sqrtPriceLimitX96 = zeroForOne ? MIN_SQRT_RATIO : MAX_SQRT_RATIO;
    }

    function getId(UniSwapCallBuffer memory self) internal pure returns (PoolId id) {
        assembly ("memory-safe") {
            id := keccak256(add(self, POOL_KEY_OFFSET), POOL_KEY_SIZE)
        }
    }

    function call(UniSwapCallBuffer memory self, IPoolManager uni) internal {
        assembly ("memory-safe") {
            let success :=
                call(gas(), uni, 0, add(self, CALL_PAYLOAD_START_OFFSET), CALL_PAYLOAD_CD_BYTES, 0, 0)
            if iszero(success) {
                let free := mload(0x40)
                returndatacopy(free, 0, returndatasize())
                return(0, returndatasize())
            }
        }
    }
}
