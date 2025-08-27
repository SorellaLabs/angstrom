# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Angstrom L2 is a Uniswap V4 hook-based MEV protection system for rollups with priority fee ordering. Unlike the original Angstrom, L2 operates entirely on-chain through hooks, leveraging the rollup's priority fee auction mechanism for MEV protection.

## Key Architecture Changes (L2 vs Original)

- **Hook-Only Design**: No off-chain components or custom order types - all functionality through Uniswap V4 hooks
- **On-Chain ToB Calculation**: Top-of-block bid calculation happens entirely within the hook
- **Priority Fee MEV Tax**: Protection via `tax = priority_fee * assumed_gas * tax_multiple`
- **Rollup-Specific**: Only deployable to rollups adhering to priority fee ordering

## Key Commands

### Build & Test

```bash
# Setup (one-time)
uv venv .venv
source .venv/bin/activate
uv pip install -r requirements.txt

# Build
forge build

# Test (FFI required for Python scripts)
forge test --ffi                      # Run all tests
forge test -vvv --ffi                  # Verbose output
forge test --match-test <name> --ffi  # Run specific test
forge test --match-contract AngstromL2 --ffi  # Test L2 implementation

# Format
forge fmt                              # Format Solidity code
forge fmt --check                      # Check formatting

# Gas snapshots
forge snapshot --ffi                   # Generate gas snapshots
forge snapshot --diff --ffi           # Compare against .forge-snapshots
```

## Angstrom L2 Implementation

### Core Contract (`src/AngstromL2.sol`)
- Implements `IBeforeSwapHook` and `IAfterSwapHook` from Uniswap V4
- Inherits from `UniConsumer` for Uniswap integration
- Hook permissions configured for MEV tax collection

### MEV Tax Mechanism
```solidity
SWAP_TAXED_GAS = 100_000        // Fixed gas estimate for swaps
SWAP_MEV_TAX_FACTOR = 49        // Tax rate = 49/50 = 98%
tax = priority_fee * SWAP_TAXED_GAS * SWAP_MEV_TAX_FACTOR
```

### Hook Permissions Required
- `beforeInitialize`: Constrain to ETH pools
- `beforeSwap`: Tax ToB transactions
- `afterSwap`: Distribute rewards
- `beforeSwapReturnDelta`: Charge MEV tax
- `afterAddLiquidity/afterRemoveLiquidity`: Tax JIT liquidity
- `afterAddLiquidityReturnDelta/afterRemoveLiquidityReturnDelta`: Charge JIT MEV tax

### Key Functions
- `beforeSwap()`: Calculates and charges MEV tax on first swap of block
- `afterSwap()`: Distributes collected tax to LPs
- `_getSwapTaxAmount()`: Calculates tax based on priority fee


## Reusable Components from Original Angstrom

### Modules (`src/modules/`)
- **UniConsumer.sol**: Base Uniswap V4 integration (already used)
- **PoolUpdates.sol**: Pool state management (may be adapted)

### Libraries (`src/libraries/`)
- **TickLib.sol**: Tick math utilities
- **X128MathLib.sol**: Fixed-point math for rewards
- **RayMathLib.sol**: Ray math for precise calculations

### Types (`src/types/`)
- **PoolRewards.sol**: Reward calculation logic (adaptable for tax distribution)
- **Asset.sol**: Asset pair representation

## Design Principles

### Security
- The implementation being sound and airtight is the primary concern, everything else is secondary
- Avoid inline assembly unless specified otherwise

### Test Driven Development
When implementing a new feature or component:
1. Define the minimal outer shell of your feature so that you can already define tests
2. Define any helpers that may be useful for testing the feature (for comparing structs, seeding/mocking examples, etc.)
3. Write unit tests for the feature including edge cases