// SPDX-License-Identifier: MIT
pragma solidity ^0.8.4;

import {IPoolManager} from "v4-core/src/interfaces/IPoolManager.sol";
import {Slot0} from "v4-core/src/types/Slot0.sol";
import {PoolId} from "v4-core/src/types/PoolId.sol";
import {TickLib} from "../libraries/TickLib.sol";

library IUniV4 {
    using IUniV4 for IPoolManager;
    using TickLib for uint256;

    error ExtsloadFailed();

    uint256 internal constant EXTSLOAD_SELECTOR = 0x1e2eaeaf;

    uint256 private constant _OWNER_SLOT = 0;
    uint256 private constant _PROTOCOL_FEES_SLOT = 1;
    uint256 private constant _PROTOCOL_FEE_CONTROLLER_SLOT = 2;
    uint256 private constant _IS_OPERATOR_SLOT = 3;
    uint256 private constant _BALANCE_OF_SLOT = 4;
    uint256 private constant _ALLOWANCE_SLOT = 5;
    uint256 private constant _POOLS_SLOT = 6;

    uint256 private constant _POOL_STATE_SLOT0_OFFSET = 0;
    uint256 private constant _POOL_STATE_FEE0_OFFSET = 1;
    uint256 private constant _POOL_STATE_FEE1_OFFSET = 2;
    uint256 private constant _POOL_STATE_LIQUIDITY_OFFSET = 3;
    uint256 private constant _POOL_STATE_TICKS_OFFSET = 4;
    uint256 private constant _POOL_STATE_BITMAP_OFFSET = 5;
    uint256 private constant _POOL_STATE_POSITIONS_OFFSET = 6;

    uint256 private constant _POSITION_LIQUIDITY_OFFSET = 0;
    uint256 private constant _POSITION_FEE_GROWTH_OUTSIDE0_OFFSET = 1;
    uint256 private constant _POSITION_FEE_GROWTH_OUTSIDE1_OFFSET = 2;

    function gudExtsload(IPoolManager self, uint256 slot)
        internal
        view
        returns (uint256 rawValue)
    {
        assembly ("memory-safe") {
            mstore(0x20, slot)
            mstore(0x00, EXTSLOAD_SELECTOR)
            if iszero(staticcall(gas(), self, 0x1c, 0x24, 0x00, 0x20)) {
                mstore(0x00, 0x535cf94b /* ExtsloadFailed() */ )
                revert(0x1c, 0x04)
            }
            rawValue := mload(0x00)
        }
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
    function computeBitmapWordSlot(IPoolManager, PoolId id, int16 wordPos)
        internal
        pure
        returns (uint256 slot)
    {
        assembly ("memory-safe") {
            // Pool state slot.
            mstore(0x00, id)
            mstore(0x20, _POOLS_SLOT)
            slot := keccak256(0x00, 0x40)
            // Compute relative map slot (Note: assumes `wordPos` is sanitized i.e. sign extended).
            mstore(0x00, wordPos)
            mstore(0x20, add(slot, _POOL_STATE_BITMAP_OFFSET))
            slot := keccak256(0x00, 0x40)
        }
    }

    function getSlot0(IPoolManager self, PoolId id) internal view returns (Slot0) {
        uint256 slot = self.computePoolStateSlot(id);
        return Slot0.wrap(bytes32(self.gudExtsload(slot)));
    }

    /**
     * @dev WARNING: use of this method with a dirty `int16` for `wordPos` may be vulnerable as the
     * value is taken as is and used in assembly. If not sign extended will result in useless slots.
     */
    function getPoolBitmapInfo(IPoolManager self, PoolId id, int16 wordPos)
        internal
        view
        returns (uint256)
    {
        uint256 slot = self.computeBitmapWordSlot(id, wordPos);
        return self.gudExtsload(slot);
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
        assembly ("memory-safe") {
            // Pool state slot derivation.
            mstore(0x20, _POOLS_SLOT)
            mstore(0x00, id)
            // Compute relative map slot (WARNING: assumes `tick` is sanitized i.e. sign extended).
            mstore(0x20, add(keccak256(0x00, 0x40), _POOL_STATE_TICKS_OFFSET))
            mstore(0x00, tick)
            // Encode calldata.
            mstore(0x20, keccak256(0x00, 0x40))
            mstore(0x00, EXTSLOAD_SELECTOR)
            if iszero(staticcall(gas(), self, 0x1c, 0x24, 0x00, 0x20)) {
                mstore(0x00, 0x535cf94b /* ExtsloadFailed() */ )
                revert(0x1c, 0x04)
            }
            let packed := mload(0x00)
            liquidityGross := shr(128, shl(128, packed))
            liquidityNet := sar(128, packed)
        }
    }

    function getPositionLiquidity(IPoolManager self, PoolId id, bytes32 positionKey)
        internal
        view
        returns (uint128 liquidity)
    {
        assembly ("memory-safe") {
            // Pool state slot.
            mstore(0x20, _POOLS_SLOT)
            mstore(0x00, id)
            // Position state slot.
            mstore(0x20, add(keccak256(0x00, 0x40), _POOL_STATE_POSITIONS_OFFSET))
            mstore(0x00, positionKey)
            // Inilined gudExtsload.
            mstore(0x20, keccak256(0x00, 0x40))
            mstore(0x00, EXTSLOAD_SELECTOR)
            if iszero(staticcall(gas(), self, 0x1c, 0x24, 0x00, 0x20)) {
                mstore(0x00, 0x535cf94b /* ExtsloadFailed() */ )
                revert(0x1c, 0x04)
            }
            liquidity := and(0xffffffffffffffffffffffffffffffff, mload(0x00))
        }
    }

    function getPoolLiquidity(IPoolManager self, PoolId id) internal view returns (uint128) {
        uint256 slot = self.computePoolStateSlot(id);
        unchecked {
            uint256 rawLiquidity = self.gudExtsload(slot + _POOL_STATE_LIQUIDITY_OFFSET);
            return uint128(rawLiquidity);
        }
    }

    function getDelta(IPoolManager self, address owner, address asset)
        internal
        view
        returns (int256 delta)
    {
        bytes32 tslot;
        assembly ("memory-safe") {
            mstore(0x00, owner)
            mstore(0x20, asset)
            tslot := keccak256(0x00, 0x40)
        }
        bytes32 value = self.exttload(tslot);
        delta = int256(uint256(value));
    }

    function isInitialized(IPoolManager self, PoolId id, int24 tick, int24 tickSpacing)
        internal
        view
        returns (bool initialized)
    {
        (int16 wordPos, uint8 bitPos) = TickLib.position(TickLib.compress(tick, tickSpacing) - 1);
        initialized = self.getPoolBitmapInfo(id, wordPos).isInitialized(bitPos);
    }

    /// @dev Gets the next tick down such that `tick ∉ [nextTick; nextTick + TICK_SPACING)`
    function getNextTickLt(IPoolManager self, PoolId id, int24 tick, int24 tickSpacing)
        internal
        view
        returns (bool initialized, int24 nextTick)
    {
        (int16 wordPos, uint8 bitPos) = TickLib.position(TickLib.compress(tick, tickSpacing) - 1);
        (initialized, bitPos) = self.getPoolBitmapInfo(id, wordPos).nextBitPosLte(bitPos);
        nextTick = TickLib.toTick(wordPos, bitPos, tickSpacing);
    }

    function getNextTickLe(IPoolManager self, PoolId id, int24 tick, int24 tickSpacing)
        internal
        view
        returns (bool initialized, int24 nextTick)
    {
        (int16 wordPos, uint8 bitPos) = TickLib.position(TickLib.compress(tick, tickSpacing));
        (initialized, bitPos) = self.getPoolBitmapInfo(id, wordPos).nextBitPosLte(bitPos);
        nextTick = TickLib.toTick(wordPos, bitPos, tickSpacing);
    }

    function getNextTickGt(IPoolManager self, PoolId id, int24 tick, int24 tickSpacing)
        internal
        view
        returns (bool initialized, int24 nextTick)
    {
        (int16 wordPos, uint8 bitPos) = TickLib.position(TickLib.compress(tick, tickSpacing) + 1);
        (initialized, bitPos) = self.getPoolBitmapInfo(id, wordPos).nextBitPosGte(bitPos);
        nextTick = TickLib.toTick(wordPos, bitPos, tickSpacing);
    }
}
