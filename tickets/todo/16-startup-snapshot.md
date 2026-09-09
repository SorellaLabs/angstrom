# 16 — Startup snapshot and queued updates

**Blocks on:** 14

## Files
- `crates/eth/src/manager.rs` — subscription and startup ordering
- `bin/angstrom/src/components.rs` — init block

## Goal
Have valid config before the first round.

## Do
- Subscribe to canonical updates *before* taking the startup snapshot, then reconcile the updates
  queued in between.
- Initialize tracking at the node's init block in `bin/angstrom/src/components.rs`.

## Done when
- A node started mid-chain has a snapshot pinned to its init block, with no gap or double-apply.
