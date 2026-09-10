# 25 — Identify async work by parent hash and generation

**Blocks on:** 23

## Files
- `crates/consensus/src/rounds/mod.rs`
- `crates/consensus/src/rounds/proposal.rs`

## Goal
Discard results that no longer belong to the current round.

## Do
- Tag async work with parent hash plus a round generation that changes on reset.
- Discard any result whose identity no longer matches. Matching block height is not enough —
  same-height reorgs exist.
- Re-check identity after async preparation, before signing, and before each endpoint send.

## Done when
- Changing the head mid-round, including to a same-height block, rejects the stale result.
