# op-angstrom

Angstrom rollup implementation for Optimism-based chains.

## Overview

This binary implements the Angstrom MEV protection protocol as a rollup sidecar, designed to work with Optimism-compatible L2 chains like Base, Unichain, and their testnets.

## Features

- **Single-node operation**: No consensus required, relies on rollup sequencer
- **RollupMode**: Uses `RollupHandles` instead of `ConsensusHandles` for inter-component communication
- **L2 Integration**: Submits bundles directly to the L2 chain via the sequencer
- **Order Management**: Accepts and processes limit orders through RPC interface

## Usage

```bash
# Run on Base Sepolia
cargo run --bin op-angstrom -- \
  --rollup.sequencer https://sepolia.base.org \
  --chain base-sepolia \
  --signer-type local \
  --private-key 0x...

# Run on Unichain Sepolia
cargo run --bin op-angstrom -- \
  --rollup.sequencer https://sepolia-rpc.unichain.org \
  --chain unichain-sepolia \
  --signer-type local \
  --private-key 0x...
```

## Configuration

Key configuration differences from consensus angstrom:
- `--rollup.sequencer`: HTTP(S) URL of the L2 sequencer (required)
- No validator set or consensus configuration needed
- Uses rollup-specific handles and primitives

## Testing

End-to-end tests are available to validate the rollup integration:

```bash
# Run e2e tests
cargo test --package op-angstrom --test e2e_rollup

# Run specific test
cargo test rollup_agent_can_generate_orders
```

The tests include:
- Order generation and submission validation
- Bundle detection in rollup blocks
- RPC interface testing

## Architecture

The rollup implementation differs from the consensus version:
- Uses `RollupMode` instead of `ConsensusMode`
- Single node operation (no peer networking)
- Direct L2 submission via sequencer
- Simplified lifecycle management

## Supported Chains

- Base (`base`)
- Base Sepolia (`base-sepolia`)
- Unichain (`unichain`)
- Unichain Sepolia (`unichain-sepolia`)
