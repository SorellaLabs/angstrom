# 22 — Pin submission-time estimate_gas

**Blocks on:** 21

## Files
- `crates/types/src/submission/mod.rs` — `estimate_gas`
- `crates/consensus/src/rounds/proposal.rs` — submission path

## Goal
Gas estimation and construction see the same state.

## Do
- Submission-time `estimate_gas` uses the same parent state and H+1 environment as the round.

## Done when
- Estimation cannot silently run against current state.
