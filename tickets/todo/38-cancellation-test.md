# 38 — Cancellation

**Blocks on:** 26

## Files
- `crates/consensus/src/rounds/proposal.rs`

## Goal
Prove a reset actually stops work.

## Do
- Invalidate a round while matching, simulation, signing, or an endpoint send is in flight.
- Assert on sends that did not happen, not on the presence of a token.
- Cover the dropped-join-handle case.

## Done when
- No later send or retry occurs in any of those phases.
