# 22 — One snapshot per round

**Blocks on:** 16

## Files
- `crates/consensus/src/manager.rs:129` — `on_blockchain_state`
- `crates/consensus/src/rounds/mod.rs:190` — `SharedRoundState`
- `crates/consensus/src/rounds/mod.rs:277` — `matching_engine_output`

## Goal
Hold the current rates in memory and fix them for the whole round.

## Do
- Hold the latest `DonationSplitSnapshot` on `SharedRoundState`, seeded at init (16) and updated
  from `EthEvent::ProtocolFeeConfigUpdated` in `on_blockchain_state`, beside the existing
  `NewBlock` handling.
- Capture it once per round in `matching_engine_output`, next to the existing
  `let pool_snapshots = self.fetch_pool_snapshot();` at `:336`. That call site is above both
  consumers, so one capture covers gas estimation and final construction.
- No provider call on this path. Block sync already guarantees the cleanser has applied the
  block's logs before the round runs.

## Done when
- Gas estimation and final construction use the same value.
- An update arriving mid-round does not change the bundle being built.

## Notes
`SharedRoundState::protocol_fee_config` stops being dead: `ConsensusManager::on_blockchain_state`
now has a `ProtocolFeeConfigUpdated` arm that writes it through
`RoundStateMachine::update_protocol_fee_config` and returns without resetting, beside the
`AddedNode` / `RemovedNode` arms. The cleanser calls `apply_periphery_logs` *before* it sends
`NewBlock` in both `handle_commit` and `handle_reorg`, so the update is already applied by the
time the same notification's block opens a round — the new round starts from it, and no provider
call is needed on this path.

The capture sits next to `fetch_pool_snapshot()` in `matching_engine_output` and rides out on the
future's own result as `MatchingOutput` — `(Vec<PoolSolution>, BundleGasDetails,
DonationSplitSnapshot)`. Carrying it on the result rather than stashing it somewhere is what makes
"the same value" structural: there is one capture in one tuple, so the value matching was driven
on and the value final construction reads cannot diverge, rather than being two reads that happen
to agree. It lands in `try_build_proposal` as `_splits`, in scope at the `from_proposal` call
ticket 26 will thread it into; it is deliberately *not* held on `ProposalState`, which would be a
second copy of something already in scope where it is needed. `FinalizationState` ignores the
extra element.

Coverage: `a_config_update_mid_round_does_not_change_the_round_being_built` drives a capture, lands
a setter while it is in flight, and asserts the round kept what it captured *and* that the next
round starts from the update — the two halves of "done when" are one property. That both consumers
see it is structural, per above, so there is nothing separate to assert until ticket 26 gives
`from_proposal` something to do with it.

The six `rounds::tests` that use `setup_state_machine` were already failing before this ticket —
`ConsensusMetricsWrapper::new` unwraps `METRICS_ENABLED`, which no test ever set. `setup_state_machine`
now sets it to `false`, which fixes those six as well as making this ticket's test possible.
