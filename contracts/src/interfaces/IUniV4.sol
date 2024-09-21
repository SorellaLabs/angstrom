// SPDX-License-Identifier: MIT
pragma solidity ^0.8.4;

import {IPoolManager} from "v4-core/src/interfaces/IPoolManager.sol";
import {Slot0} from "v4-core/src/types/Slot0.sol";
import {PoolId} from "v4-core/src/types/PoolId.sol";
import {TickLib} from "../libraries/TickLib.sol";

library IUniV4 {
    using IUniV4 for IPoolManager;
    using TickLib for uint256;

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

    uint256 internal constant _POSITION_LIQUIDITY_OFFSET = 0;
    uint256 internal constant _POSITION_FEE_GROWTH_OUTSIDE0_OFFSET = 1;
    uint256 internal constant _POSITION_FEE_GROWTH_OUTSIDE1_OFFSET = 2;

    function extsload(IPoolManager self, uint256 slot) internal view returns (uint256 rawValue) {
        rawValue = uint256(self.extsload(bytes32(slot)));
    }

    function computePoolStateSlot(IPoolManager, PoolId id) internal pure returns (uint256 slot) {
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
    function computeBitmapWordSlot(IPoolManager, PoolId id, int16 wordPos) internal pure returns (uint256 slot) {
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

    function computePositionStateSlot(IPoolManager self, PoolId id, bytes32 positionKey)
        internal
        pure
        returns (uint256 slot)
    {
        uint256 poolStateSlot = self.computePoolStateSlot(id);
        assembly ("memory-safe") {
            mstore(0x00, positionKey)
            mstore(0x20, poolStateSlot)
            slot := keccak256(0x00, 0x40)
        }
    }

    function getSlot0(IPoolManager self, PoolId id) internal view returns (Slot0) {
        uint256 slot = self.computePoolStateSlot(id);
        return Slot0.wrap(self.extsload(bytes32(slot)));
    }

    /**
     * @dev WARNING: use of this method with a dirty `int16` for `wordPos` may be vulnerable as the
     * value is taken as is and used in assembly. If not sign extended will result in useless slots.
     */
    function getPoolBitmapInfo(IPoolManager self, PoolId id, int16 wordPos) internal view returns (uint256) {
        uint256 slot = self.computeBitmapWordSlot(id, wordPos);
        return self.extsload(slot);
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

    function getPositionLiquidity(IPoolManager self, PoolId id, bytes32 positionKey) internal view returns (uint128) {
        unchecked {
            uint256 positionStateSlot = self.computePositionStateSlot(id, positionKey);
            uint256 rawLiquidity = self.extsload(positionStateSlot + _POSITION_LIQUIDITY_OFFSET);
            return uint128(rawLiquidity);
        }
    }

    function getPoolLiquidity(IPoolManager self, PoolId id) internal view returns (uint128) {
        uint256 slot = self.computePoolStateSlot(id);
        unchecked {
            uint256 rawLiquidity = self.extsload(slot + _POOL_STATE_LIQUIDITY_OFFSET);
            return uint128(rawLiquidity);
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
        delta = int256(uint256(value));
    }

    function isInitialized(IPoolManager self, PoolId id, int24 tick) internal view returns (bool initialized) {
        (int16 wordPos, uint8 bitPos) = TickLib.position(TickLib.compress(tick) - 1);
        initialized = self.getPoolBitmapInfo(id, wordPos).isInitialized(bitPos);
    }

    /// @dev Gets the next tick down such that `tick ∉ [nextTick; nextTick + TICK_SPACING)`
    function getNextTickLt(IPoolManager self, PoolId id, int24 tick)
        internal
        view
        returns (bool initialized, int24 nextTick)
    {
        (int16 wordPos, uint8 bitPos) = TickLib.position(TickLib.compress(tick) - 1);
        (initialized, bitPos) = self.getPoolBitmapInfo(id, wordPos).nextBitPosLte(bitPos);
        nextTick = TickLib.toTick(wordPos, bitPos);
    }

    function getNextTickLe(IPoolManager self, PoolId id, int24 tick)
        internal
        view
        returns (bool initialized, int24 nextTick)
    {
        (int16 wordPos, uint8 bitPos) = TickLib.position(TickLib.compress(tick));
        (initialized, bitPos) = self.getPoolBitmapInfo(id, wordPos).nextBitPosLte(bitPos);
        nextTick = TickLib.toTick(wordPos, bitPos);
    }

    function getNextTickGt(IPoolManager self, PoolId id, int24 tick)
        internal
        view
        returns (bool initialized, int24 nextTick)
    {
        (int16 wordPos, uint8 bitPos) = TickLib.position(TickLib.compress(tick) + 1);
        (initialized, bitPos) = self.getPoolBitmapInfo(id, wordPos).nextBitPosGte(bitPos);
        nextTick = TickLib.toTick(wordPos, bitPos);
    }
}
