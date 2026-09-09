# 23 — One snapshot per round

**Blocks on:** 14, 16

## Files
- `crates/consensus/src/manager.rs`
- `crates/consensus/src/rounds/mod.rs`
- `crates/consensus/src/rounds/proposal.rs`

## Goal
Capture the rates once at parent H and hold them for the whole round.

## Do
- `crates/consensus/src/manager.rs`, `rounds/mod.rs`, `rounds/proposal.rs`.
- Take one `DonationSplitSnapshot` per round and reuse it for matching, gas estimation, and final
  construction.
- Retain the round's pool snapshots too, rather than re-fetching mutable pool state for final
  construction.

## Done when
- No re-read of config or pool state between estimation and construction.
- A setter landing in H+1 does not affect the bundle built on H.
