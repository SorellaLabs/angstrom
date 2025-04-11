//SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {IPoolManager} from "v4-core/src/interfaces/IPoolManager.sol";
import {TickMath} from "v4-core/src/libraries/TickMath.sol";
import {PoolId} from "v4-core/src/types/PoolId.sol";
import {IUniV4} from "core/src/interfaces/IUniV4.sol";

contract GetUniswapV4TickData {
    struct TickData {
        bool initialized;
        int24 tick;
        uint128 liquidityGross;
        int128 liquidityNet;
    }

    struct TicksWithBlock {
        TickData[] ticks;
        uint256 blockNumber;
    }

    constructor(
        PoolId poolId,
        address poolManager,
        bool zeroForOne,
        int24 currentTick,
        uint16 numTicks,
        int24 tickSpacing
    ) {
        TickData[] memory tickData = new TickData[](numTicks);

        //Instantiate current word position to keep track of the word count
        uint256 counter = 0;

        while (counter < numTicks) {
            (bool initialized, int24 nextTick) = zeroForOne
                ? IUniV4.getNextTickLe(
                    IPoolManager(poolManager),
                    poolId,
                    currentTick,
                    tickSpacing
                )
                : IUniV4.getNextTickGt(
                    IPoolManager(poolManager),
                    poolId,
                    currentTick,
                    tickSpacing
                );

            (uint128 liquidityGross, int128 liquidityNet) = IUniV4
                .getTickLiquidity(IPoolManager(poolManager), poolId, nextTick);

            //Make sure not to overshoot the max/min tick
            //If we do, break the loop, and set the last initialized tick to the max/min tick=
            if (nextTick < TickMath.MIN_TICK) {
                tickData[counter].initialized = initialized;
                tickData[counter].tick = TickMath.MIN_TICK;
                tickData[counter].liquidityGross = liquidityGross;
                tickData[counter].liquidityNet = liquidityNet;
                break;
            } else if (nextTick >= TickMath.MAX_TICK) {
                tickData[counter].initialized = initialized;
                tickData[counter].tick = TickMath.MAX_TICK;
                tickData[counter].liquidityGross = liquidityGross;
                tickData[counter].liquidityNet = liquidityNet;
                break;
            } else {
                tickData[counter].initialized = initialized;
                tickData[counter].tick = nextTick;
                tickData[counter].liquidityGross = liquidityGross;
                tickData[counter].liquidityNet = liquidityNet;
            }

            counter++;

            currentTick = nextTick;
            if (zeroForOne) {
                --currentTick;
            }
        }

        TicksWithBlock memory ticksWithBlock = TicksWithBlock({
            ticks: tickData,
            blockNumber: block.number
        });

        // ensure abi encoding, not needed here but increase reusability for different return types
        // note: abi.encode add a first 32 bytes word with the address of the original data
        bytes memory abiEncodedData = abi.encode(ticksWithBlock);

        assembly {
            let dataStart := add(abiEncodedData, 0x20)
            let dataSize := mload(abiEncodedData)
            return(dataStart, dataSize)
        }
    }

    // function nextBitPosLte(
    //     uint256 word,
    //     uint8 bitPos
    // ) internal pure returns (bool initialized, uint8 nextBitPos) {
    //     unchecked {
    //         uint8 offset = 0xff - bitPos;
    //
    //         uint256 relativePos = LibBit.fls(word << offset);
    //         initialized = relativePos != 256;
    //         nextBitPos = initialized ? uint8(relativePos - offset) : bitPos;
    //     }
    // }
    //
    // function nextBitPosGte(
    //     uint256 word,
    //     uint8 bitPos
    // ) internal pure returns (bool initialized, uint8 nextBitPos) {
    //     unchecked {
    //         uint256 relativePos = LibBit.ffs(word >> bitPos);
    //         initialized = relativePos != 256;
    //         nextBitPos = initialized
    //             ? uint8(relativePos + bitPos)
    //             : type(uint8).max - bitPos;
    //     }
    // }
    //
    // function isInitialized(
    //     uint256 word,
    //     uint8 bitPos
    // ) internal pure returns (bool) {
    //     return word & (uint256(1) << bitPos) != 0;
    // }
    //
    // function getNextTickLtQuery(
    //     IPoolManager self,
    //     PoolId id,
    //     int24 tick,
    //     int24 tickSpacing
    // ) internal view returns (bool initialized, int24 nextTick) {
    //     (int16 wordPos, uint8 bitPos) = TickLib.position(
    //         TickLib.compress(tick, tickSpacing) - 1
    //     );
    //     uint word = getPoolBitmapInfo(self, id, wordPos);
    //
    //     (initialized, bitPos) = nextBitPosLte(word, bitPos);
    //     nextTick = TickLib.toTick(wordPos, bitPos, tickSpacing);
    // }
    //
    // function getNextTickGtQuery(
    //     IPoolManager self,
    //     PoolId id,
    //     int24 tick,
    //     int24 tickSpacing
    // ) internal view returns (bool initialized, int24 nextTick) {
    //     (int16 wordPos, uint8 bitPos) = TickLib.position(
    //         TickLib.compress(tick, tickSpacing) + 1
    //     );
    //     uint word = getPoolBitmapInfo(self, id, wordPos);
    //
    //     (initialized, bitPos) = nextBitPosGte(word, bitPos);
    //     nextTick = TickLib.toTick(wordPos, bitPos, tickSpacing);
    // }
    //
    // function getPoolBitmapInfo(
    //     IPoolManager self,
    //     PoolId id,
    //     int16 wordPos
    // ) internal view returns (uint256) {
    //     uint256 slot = computeBitmapWordSlot(self, id, wordPos);
    //     return gudExtsload(self, slot);
    // }
    //
    // uint256 internal constant EXTSLOAD_SELECTOR = 0x1e2eaeaf;
    //
    // function gudExtsload(
    //     IPoolManager self,
    //     uint256 slot
    // ) internal view returns (uint256 rawValue) {
    //     assembly ("memory-safe") {
    //         mstore(0x20, slot)
    //         mstore(0x00, EXTSLOAD_SELECTOR)
    //         if iszero(staticcall(gas(), self, 0x1c, 0x24, 0x00, 0x20)) {
    //             mstore(0x00, 0x535cf94b /* ExtsloadFailed() */)
    //             revert(0x1c, 0x04)
    //         }
    //         rawValue := mload(0x00)
    //     }
    // }
    //
    // uint256 private constant _POOLS_SLOT = 6;
    // uint256 private constant _POOL_STATE_BITMAP_OFFSET = 5;
    //
    // function computeBitmapWordSlot(
    //     IPoolManager,
    //     PoolId id,
    //     int16 wordPos
    // ) internal pure returns (uint256 slot) {
    //     assembly ("memory-safe") {
    //         // Pool state slot.
    //         mstore(0x00, id)
    //         mstore(0x20, _POOLS_SLOT)
    //         slot := keccak256(0x00, 0x40)
    //         // Compute relative map slot (Note: assumes `wordPos` is sanitized i.e. sign extended).
    //         mstore(0x00, wordPos)
    //         mstore(0x20, add(slot, _POOL_STATE_BITMAP_OFFSET))
    //         slot := keccak256(0x00, 0x40)
    //     }
    // }
}
