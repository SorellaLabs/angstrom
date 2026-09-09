# 24 — Carry the snapshot through the matching engine

**Blocks on:** 23

## Files
- `crates/matching-engine/src/lib.rs` — `solve_pools`
- `crates/matching-engine/src/manager.rs` — `MatcherCommand::BuildProposal`

## Goal
Get the round's splits to where the bundle is built.

## Do
- Thread `DonationSplitSnapshot` through `solve_pools` in `crates/matching-engine/src/lib.rs` and
  `MatcherCommand::BuildProposal` in `crates/matching-engine/src/manager.rs`.

## Done when
- The splits reach `process_solution` without a second read.
