// SPDX-License-Identifier: MIT
pragma solidity ^0.8.4;

import {IPoolManager} from "v4-core/src/interfaces/IPoolManager.sol";
import {Slot0} from "v4-core/src/types/Slot0.sol";
import {PoolId} from "v4-core/src/types/PoolId.sol";
import {ConversionLib} from "../libraries/ConversionLib.sol";
import {AssetIndex} from "../libraries/PriceGraph.sol";
import {Globals} from "../libraries/Globals.sol";

library IUniV4 {
    using IUniV4 for IPoolManager;

    uint256 internal constant _OWNER_SLOT = 0;
    uint256 internal constant _PROTOCOL_FEES_SLOT = 1;
    uint256 internal constant _PROTOCOL_FEE_CONTROLLER_SLOT = 2;
    uint256 internal constant _IS_OPERATOR_SLOT = 3;
    uint256 internal constant _BALANCE_OF_SLOT = 4;
    uint256 internal constant _ALLOWANCE_SLOT = 5;
    uint256 internal constant _POOLS_SLOT = 6;

    uint256 internal constant _POOL_STATE_SLOT0_OFFSET = 0;
    uint256 internal constant _POOL_STATE_FEE0_OFFSET = 1;
    uint256 internal constant _POOL_STATE_FEE1_OFFSET = 2;
    uint256 internal constant _POOL_STATE_LIQUIDITY_OFFSET = 3;
    uint256 internal constant _POOL_STATE_TICKS_OFFSET = 4;
    uint256 internal constant _POOL_STATE_BITMAP_OFFSET = 5;
    uint256 internal constant _POOL_STATE_POSITIONS_OFFSET = 6;

    /// @dev Uniswap's `MIN_SQRT_RATIO + 1` to pass the limit check.
    uint160 internal constant MIN_SQRT_RATIO = 4295128740;
    /// @dev Uniswap's `MAX_SQRT_RATIO - 1` to pass the limit check.
    uint160 internal constant MAX_SQRT_RATIO = 1461446703485210103287273052203988822378723970341;

    struct Swap {
        AssetIndex asset0Index;
        AssetIndex asset1Index;
        bool zeroForOne;
        uint256 amountIn;
    }

    function swap(IPoolManager self, Swap memory params, Globals memory g) internal {
        self.swap(
            ConversionLib.toPoolKey(address(this), g.get(params.asset0Index), g.get(params.asset1Index)),
            IPoolManager.SwapParams({
                zeroForOne: params.zeroForOne,
                amountSpecified: ConversionLib.neg(params.amountIn),
                sqrtPriceLimitX96: params.zeroForOne ? MIN_SQRT_RATIO : MAX_SQRT_RATIO
            }),
            ""
        );
    }

    function computePoolStateSlot(IPoolManager, PoolId id) internal pure returns (bytes32 slot) {
        assembly ("memory-safe") {
            mstore(0x00, id)
            mstore(0x20, _POOLS_SLOT)
            slot := keccak256(0x00, 0x40)
        }
    }

    /**
     * @dev WARNING: use of this method with a dirty `int16` for `wordPos` may be vulnerable as the
     * value is taken as is and used in assembly. If not sign extended will result in useless slots.
     */
    function computeBitmapWordSlot(IPoolManager, PoolId id, int16 wordPos) internal pure returns (bytes32 slot) {
        assembly ("memory-safe") {
            mstore(0x00, id)
            mstore(0x20, _POOLS_SLOT)
            // Pool state slot.
            slot := keccak256(0x00, 0x40)
            // Compute relative map slot (Note: assumes `wordPos` is sanitized i.e. sign extended).
            mstore(0x00, wordPos)
            mstore(0x20, add(slot, _POOL_STATE_BITMAP_OFFSET))
            slot := keccak256(0x00, 0x40)
        }
    }

    /**
     * @dev WARNING: Calling this method without first sanitizing `tick` (to ensure it's sign
     * extended) is unsafe.
     */
    function computeTickInfoSlot(IPoolManager, PoolId id, int24 tick) internal pure returns (bytes32 slot) {
        assembly ("memory-safe") {
            mstore(0x00, id)
            mstore(0x20, _POOLS_SLOT)
            // Pool state slot.
            slot := keccak256(0x00, 0x40)
            // Compute relative map slot (WARNING: assumes `tick` is sanitized i.e. sign extended).
            mstore(0x00, tick)
            mstore(0x20, add(slot, _POOL_STATE_TICKS_OFFSET))
            slot := keccak256(0x00, 0x40)
        }
    }

    function getSlot0(IPoolManager self, PoolId id) internal view returns (Slot0) {
        bytes32 slot = self.computePoolStateSlot(id);
        return Slot0.wrap(self.extsload(slot));
    }

    /**
     * @dev WARNING: use of this method with a dirty `int16` for `wordPos` may be vulnerable as the
     * value is taken as is and used in assembly. If not sign extended will result in useless slots.
     */
    function getPoolBitmapInfo(IPoolManager self, PoolId id, int16 wordPos) internal view returns (uint256) {
        bytes32 slot = self.computeBitmapWordSlot(id, wordPos);
        return uint256(self.extsload(slot));
    }

    /**
     * @dev WARNING: Calling this method without first sanitizing `tick` (to ensure it's sign
     * extended) is unsafe.
     */
    function getTickLiquidity(IPoolManager self, PoolId id, int24 tick)
        internal
        view
        returns (uint128 liquidityGross, int128 liquidityNet)
    {
        bytes32 slot = self.computeTickInfoSlot(id, tick);
        bytes32 packed = self.extsload(slot);
        assembly {
            liquidityGross := shr(128, shl(128, packed))
            liquidityNet := sar(128, packed)
        }
    }

    function getPoolLiquidity(IPoolManager self, PoolId id) internal view returns (uint128) {
        bytes32 slot = self.computePoolStateSlot(id);
        unchecked {
            bytes32 liquidity = self.extsload(bytes32(uint256(slot) + _POOL_STATE_LIQUIDITY_OFFSET));
            return uint128(uint256(liquidity));
        }
    }

    function getDelta(IPoolManager self, address owner, address asset) internal view returns (int256 delta) {
        bytes32 tslot;
        assembly ("memory-safe") {
            mstore(0x00, owner)
            mstore(0x20, asset)
            tslot := keccak256(0x00, 0x40)
        }
        bytes32 value = self.exttload(tslot);
        assembly {
            // Direct type cast.
            delta := value
        }
    }
}
