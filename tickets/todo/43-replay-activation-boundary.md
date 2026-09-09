# 43 — Replay either side of A

**Blocks on:** 12, 28

## Files
- `bin/replay/src/lib.rs`
- `testing-tools/src/replay/`
- `testing-tools/src/types/config/replay.rs`
- `crates/types/src/traits/bundles.rs` — legacy vs. current path

## Goal
Keep historical replay byte-exact.

## Do
- Before **A**: keep the legacy `f64` path, full ToB budget, and legacy allocation behavior.
- At or after **A**: load rates from historical parent state.
- Missing historical state is a reported gap, never a silent use of today's rate.

## Done when
- A block either side of **A** replays identically to what was included.
