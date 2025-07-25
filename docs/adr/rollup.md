# Rollup Achitectural Decision Record

This document outlines the architectural decisions made for the rollup mode of angstrom: `op-angstrom`.

## High-Level
- A new binary for rollup mode: [`op-angstrom`](../../bin/op-angstrom)
- Crates using default Eth L1 primitives are made generic over [`NodePrimitives`](https://reth.rs/docs/reth_primitives_traits/node/trait.NodePrimitives.html)
    - This trait is a template for the most important chain-related primitives used in Angstrom: `Block`, `BlockHeader`, `BlockBody`, `SignedTx`, `Receipt`
    - The default implementation is always [`EthPrimitives`](https://reth.rs/docs/reth/primitives/struct.EthPrimitives.html)
    - If needed, can be overridden to use [`OpPrimitives`](https://reth.rs/docs/op_reth/primitives/struct.OpPrimitives.html)
    TODO: Add which crates are affected by this
- Crates that have _different logic_ for rollup mode use the type state pattern to express that logic.
    - `ConsensusMode` and `RollupMode` are the two modes.
    - `ConsensusMode` contains consensus and networking related logic and state.
    - `RollupMode` contains rollup related logic and state.
    - We then use concrete implementations of each type state to express the logic for each mode.

## Reasoning

- We chose this approach over working with feature flags because it doesn't work as expected in workspaces (where each feature is additive, i.e. you can't import a workspace member from one binary with a specific feature flag, and then use that member in another binary with a different feature flag). 
- We chose this approach over runtime decisions (i.e., consensus handles / streams are optional and have to be configured at runtime) because that adds unnecessary overhead. We know what we need at compile time, hence we can use the type state pattern to express the logic for each mode.

The downside of this approach is that there will be some code duplication between the two modes, and just more code overall.