# 17 — A failed read skips the round

**Blocks on:** 13

## Files
- `crates/eth/src/protocol_fee_config.rs` — created by ticket 13
- `crates/types/src/reth_db_wrapper.rs` — `.latest().unwrap()` and the defaulting accessors
- `crates/consensus/src/rounds/mod.rs` — skip or retry the round

## Goal
Never build on a stale or defaulted rate.

## Do
- A failed config read skips or retries the round. No fallback to a previous or default value.
- The local provider adapter unwraps state-provider errors; turn those panics into errors.

## Done when
- With the read forced to fail, no bundle is built and no default rate is used.
