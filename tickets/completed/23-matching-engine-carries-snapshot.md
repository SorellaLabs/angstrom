# 23 — Carry the snapshot to both consumers

**Blocks on:** 22

## Files
- `crates/consensus/src/rounds/mod.rs:337` — `solve_pools` call
- `crates/consensus/src/rounds/proposal.rs:164` — `from_proposal`
- `crates/matching-engine/src/lib.rs` — `solve_pools`
- `crates/matching-engine/src/manager.rs:46` — `MatcherCommand::BuildProposal`
- `crates/matching-engine/src/manager.rs:175` — `for_gas_finalization`

## Goal
Both `process_solution` call sites get the round's one snapshot.

## Do
- Add the snapshot to `solve_pools` and to `MatcherCommand::BuildProposal`, alongside
  `pool_snapshots`, through to `for_gas_finalization`.
- Stash the same value on `ProposalState` so `try_build_proposal` passes it to `from_proposal`.
  `try_build_proposal` receives the matching result as a parameter, so it runs after the engine —
  it cannot be the capture point for both.

## Done when
- `for_gas_finalization` and `from_proposal` in one round are given the same snapshot.

## Notes
`solve_pools` now takes the round's rates and carries them through
`MatcherCommand::BuildProposal` and `build_proposal` to `for_gas_finalization`, beside
`pool_snapshots` and `parent_hash`.

**`DonationSplits`, not the whole snapshot.** The engine only ever reads the rates; it never reads
the snapshot's `block_number` or `block_hash`. Handing it the snapshot would also put a second hash
beside `parent_hash` that reads as if it were the same thing — it is not. `parent_hash` is the
round's parent **H**, which simulation pins to; a snapshot's `block_hash` is the tip at which the
configuration last *changed*, because the eth manager publishes `ProtocolFeeConfigUpdated` only
when a `LpDonationSplitsSet` log appears, and it is `B256::ZERO` pre-deployment. Two unrelated
hashes in one signature is a trap; the rates are what this code needs.

The snapshot itself stays on `MatchingOutput`, where its identity is the round's provenance and
ticket 36 will want it.

It is *not* stashed on `ProposalState` as the ticket first suggested. Ticket 22 had already decided
the other way: the snapshot rides out of `matching_engine_output` on `MatchingOutput`, so the value
the engine was driven on and the value `try_build_proposal` reads come from one local variable used
twice, rather than a copy that has to be kept in step. A second copy on `ProposalState` would be
exactly the divergence the ticket is trying to rule out.

Coverage: `a_config_update_mid_round_does_not_change_the_round_being_built` gained an assertion that
`MockMatchingEngine` was handed the same rates the round kept. The mock records what `solve_pools`
received, the same reason it already echoes `parent_hash` back — so a test cannot pass on a value
the caller never sent. The `from_proposal` half is ticket 24.
