# 24 — Carry the snapshot to both consumers

**Blocks on:** 23

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
