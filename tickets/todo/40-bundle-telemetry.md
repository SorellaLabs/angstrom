# 40 — Record per-bundle fee telemetry

**Blocks on:** 30

## Files
- `crates/telemetry-recorder/src/lib.rs`
- `crates/eth/src/telemetry.rs`
- `crates/types/src/traits/bundles.rs` — where the numbers are produced

## Goal
Feed the ledger.

## Do
- Per pool and per included bundle, record gross ToB payment, LP allocation, the configured
  protocol fee, rounding and retained-remainder buckets separately, and the snapshot identity.
- Keep the historical construction parent separate from the local round generation so replay and
  other nodes can reproduce the check.

## Done when
- A included bundle's numbers can be reconstructed from the record alone.
