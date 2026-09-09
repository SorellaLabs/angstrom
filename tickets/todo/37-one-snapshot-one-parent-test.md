# 37 — One snapshot, one parent

**Blocks on:** 23, 25

## Files
- `crates/consensus/src/rounds/mod.rs`
- `crates/consensus/src/rounds/proposal.rs`
- `testing-tools/src/mocks/canon_state.rs` — head changes and same-height reorg

## Goal
Lock down the round invariant.

## Do
- Drive a round end to end. Assert gas estimation and final construction used the same snapshot and
  parent hash, and that neither re-read config or pool state.
- Change the head mid-round, including a same-height reorg, and assert the stale result is
  rejected.

## Done when
- The test fails if a second read is introduced.
