# Rollup Architectural Decision Record

This document outlines the architectural decisions made for the rollup mode of angstrom: `op-angstrom`. 

## Overview
We introduced a new binary for rollup mode, which builds on top of `op-reth`. You can find it [here](../../bin/op-angstrom). It inherits the regular `op-reth` CLI parameters, combined with Angstrom-specific
parameters. Check out the documentation for `op-reth` [here](https://reth.rs/run/opstack).

### Types and Primitives
Initially, the codebase defaulted to using Eth L1 primitives (represented by [`EthPrimitives`](https://reth.rs/docs/reth/primitives/struct.EthPrimitives.html)). We started by making this generic over [`NodePrimitives`](https://reth.rs/docs/reth_primitives_traits/node/trait.NodePrimitives.html), which is a template for the most important chain-related primitives used in Angstrom: `Block`, `BlockHeader`, `BlockBody`, `SignedTx`, `Receipt`. The default implementation is always [`EthPrimitives`](https://reth.rs/docs/reth/primitives/struct.EthPrimitives.html), but if needed, it can be overridden to use [`OpPrimitives`](https://reth.rs/docs/op_reth/primitives/struct.OpPrimitives.html).

**Implementations**

| File | Components/Types | Notes |
|---|---|---|
| [crates/eth/src/handle.rs](../../crates/eth/src/handle.rs) | Eth<N: NodePrimitives>; EthCommand<N: NodePrimitives>; EthHandle<N: NodePrimitives> | Generic subscription/command types over primitives |
| [crates/eth/src/manager.rs](../../crates/eth/src/manager.rs) | EthDataCleanser<Sync, N: NodePrimitives> | Consumes CanonStateNotifications<N> |
| [crates/eth/src/telemetry.rs](../../crates/eth/src/telemetry.rs) | EthUpdaterSnapshot<N: NodePrimitives>; AngstromChainUpdate<N: NodePrimitives> | Telemetry generic over primitives |
| [crates/types/src/primitive/chain_ext.rs](../../crates/types/src/primitive/chain_ext.rs) | ChainExt<N: NodePrimitives> | Extension over reth Chain<N> |
| [crates/cli/src/components.rs](../../crates/cli/src/components.rs) | handle_init_block_spam<N: NodePrimitives>(...) | Utility generic over N |
| [crates/cli/src/handles.rs](../../crates/cli/src/handles.rs) | AngstromMode (Primitives: NodePrimitives) | Type-state selects primitives |

### Providers
For components that also use a provider to talk to Reth over an API (DB or RPC), we introduced a new trait in `angstrom-types` called [`NetworkProvider`](../../crates/types/src/provider.rs). It inherits from [`NodePrimitivesProvider`](https://reth.rs/docs/reth_primitives_traits/node/trait.NodePrimitivesProvider.html), but also has an associated `Network` type which is constrained to be a [`Network`](https://alloy.rs/guides/interacting-with-multiple-networks#the-network-trait) from `alloy`. This network is then used to specify the network that the provider is for.

**Implementations**

| File | Components/Types | Notes |
|---|---|---|
| [crates/types/src/submission/mod.rs](../../crates/types/src/submission/mod.rs) | TxFeatureInfo<N>; SubmissionHandler<P, N>; ChainSubmitterHolder<I, S, N> | Uses NetworkProvider to bind alloy Network and primitives |
| [crates/types/src/submission/mempool.rs](../../crates/types/src/submission/mempool.rs) | MempoolSubmitter | ChainSubmitter using Provider<N::Network> |
| [crates/types/src/submission/angstrom.rs](../../crates/types/src/submission/angstrom.rs) | AngstromSubmitter | Angstrom integration submitter |
| [crates/types/src/submission/mev_boost.rs](../../crates/types/src/submission/mev_boost.rs) | MevBoostSubmitter; BundleSigner; MevHttp | Flashbots/MEV submission path |
| [testing-tools/src/providers/anvil_submission.rs](../../testing-tools/src/providers/anvil_submission.rs) | AnvilSubmissionProvider | Test provider wrapper |
| [op-testing-tools/src/providers/anvil_submission.rs](../../op-testing-tools/src/providers/anvil_submission.rs) | AnvilSubmissionProvider | OP test provider wrapper |
| [crates/uniswap-v4/src/uniswap/pool_factory.rs](../../crates/uniswap-v4/src/uniswap/pool_factory.rs) | V4PoolFactory<P, N, const TICKS: u16> | Provider bound to N::Network |
| [crates/uniswap-v4/src/uniswap/pool_manager.rs](../../crates/uniswap-v4/src/uniswap/pool_manager.rs) | UniswapPoolManager<P, PP, N, BlockSync, const TICKS: u16>; SyncedUniswapPools; TickRangeToLoad | Network-bound pool management |
| [crates/uniswap-v4/src/uniswap/pool_providers/provider_adapter.rs](../../crates/uniswap-v4/src/uniswap/pool_providers/provider_adapter.rs) | ProviderAdapter<P, N> | Adapts alloy provider to PoolManagerProvider |
| [crates/uniswap-v4/src/uniswap/pool_providers/canonical_state_adapter.rs](../../crates/uniswap-v4/src/uniswap/pool_providers/canonical_state_adapter.rs) | CanonicalStateAdapter<P, N> | Wraps CanonStateNotifications<N::Primitives> |
| [crates/uniswap-v4/src/uniswap/pool_providers/mock_block_stream.rs](../../crates/uniswap-v4/src/uniswap/pool_providers/mock_block_stream.rs) | MockBlockStream<P, N> | Test-only block stream |
| [crates/uniswap-v4/src/uniswap/pool.rs](../../crates/uniswap-v4/src/uniswap/pool.rs) | EnhancedUniswapPool<Loader>; SwapResult | Loading via Provider<N::Network> |
| [crates/uniswap-v4/src/uniswap/pool_data_loader.rs](../../crates/uniswap-v4/src/uniswap/pool_data_loader.rs) | DataLoader; PoolData; PoolDataV4; TicksWithBlock; TickData | Data loading generics over Provider<N::Network> |
| [crates/types/src/pair_with_price.rs](../../crates/types/src/pair_with_price.rs) | PairsWithPrice | Streams CanonStateNotification<N::Primitives> and uses Provider<N::Network> |
| [crates/validation/src/common/token_pricing.rs](../../crates/validation/src/common/token_pricing.rs) | TokenPriceGenerator | Pricing via Provider<N::Network> + AMM state |

### Logic Changes
Crates that have _different logic_ for rollup mode use the type state pattern to express that logic:
- `ConsensusMode` and `RollupMode` are the two modes.
- `ConsensusMode` contains consensus and networking related logic and state.
- `RollupMode` contains rollup related logic and state.
- We then use concrete implementations of each type state to express the logic for each mode.
- Nice to have: A type alias for each mode.

| Area | File | Components/Types | Mode(s) | Notes/Aliases |
|---|---|---|---|---|
| CLI modes | [crates/cli/src/handles.rs](../../crates/cli/src/handles.rs) | AngstromMode (trait), ConsensusMode, RollupMode | Both | Aliases: ConsensusHandles, RollupHandles |
| CLI launcher | [crates/cli/src/components.rs](../../crates/cli/src/components.rs) | AngstromLauncher<N, AO, M, S> + impl for each mode | Both | Consensus wires network/consensus; Rollup is rollup-only |
| CLI entrypoint (consensus) | [crates/cli/src/angstrom.rs](../../crates/cli/src/angstrom.rs) | AngstromLauncher::<…, ConsensusMode, _>::new(...).with_network(...).with_consensus_client(...).with_node_set(...) | Consensus | Main L1 binary wiring |
| CLI entrypoint (rollup) | [crates/cli/src/op_angstrom.rs](../../crates/cli/src/op_angstrom.rs) | AngstromLauncher::<…, RollupMode, _>::new(...).launch() | Rollup | OP Stack binary wiring |
| Pool manager (consensus) | [crates/pool-manager/src/consensus.rs](../../crates/pool-manager/src/consensus.rs) | ConsensusMode; ConsensusPoolManager<V, GS>; ConsensusPoolManagerBuilder | Consensus | Networking state, peer caches, propagation |
| Pool manager (rollup) | [crates/pool-manager/src/rollup.rs](../../crates/pool-manager/src/rollup.rs) | RollupMode; RollupPoolManager<V, GS>; RollupPoolManagerBuilder | Rollup | No networking; block-sync and pool-only logic |
| Pool manager re-exports | [crates/pool-manager/src/lib.rs](../../crates/pool-manager/src/lib.rs) | pub use ConsensusMode/ConsensusPoolManager; pub use RollupMode/RollupPoolManager | Both | Convenience re-exports |
| AMM quoter (consensus) | [crates/amm-quoter/src/consensus.rs](../../crates/amm-quoter/src/consensus.rs) | ConsensusMode; ConsensusQuoterManager<BlockSync> | Consensus | Bounds order set by consensus round |
| AMM quoter (rollup) | [crates/amm-quoter/src/rollup.rs](../../crates/amm-quoter/src/rollup.rs) | RollupMode; RollupQuoterManager<BlockSync> | Rollup | Considers all orders |
| AMM quoter core | [crates/amm-quoter/src/lib.rs](../../crates/amm-quoter/src/lib.rs) | QuoterManager<BlockSync, M> (generic over mode) | Both | Mode chosen via type parameter + aliases |
