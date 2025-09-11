// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import {PoolKey} from "v4-core/src/types/PoolKey.sol";

/// @title IAngstromAdapter
/// @notice Interface for the Angstrom adapter that handles attestation selection and Uniswap v4 swaps
/// @author Jet Jadeja <jjadeja@usc.edu>
interface IAngstromAdapter {
    /// @notice Attestation data for a specific block
    struct Attestation {
        uint64 blockNumber;  // The block number this attestation is valid for
        bytes unlockData;    // 20 bytes validator address + signature bytes
    }

    /// @notice Executes a swap on an Angstrom-protected pool with attestation selection
    /// @param key The pool key identifying the pool to swap in
    /// @param zeroForOne The direction of the swap (true = token0 -> token1)
    /// @param amountIn The exact input amount
    /// @param minAmountOut The minimum output amount (slippage protection)
    /// @param bundle Array of attestations for different block numbers
    /// @param recipient The address that will receive the output tokens
    /// @param deadline The deadline for the swap to be executed
    /// @return amountOut The actual output amount received
    function swap(
        PoolKey calldata key,
        bool zeroForOne,
        uint128 amountIn,
        uint128 minAmountOut,
        Attestation[] calldata bundle,
        address recipient,
        uint256 deadline
    ) external returns (uint256 amountOut);
}