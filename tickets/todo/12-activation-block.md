# 12 — Activation block A per network

**Blocks on:** 05

## Files
- `crates/types/constants/src/lib.rs`

## Goal
Give replay and the node a fixed switchover point.

## Do
- Activation block constant per network in `crates/types/constants/src/lib.rs`, alongside the
  config address.

## Done when
- A node can ask "is block N at or after A".
