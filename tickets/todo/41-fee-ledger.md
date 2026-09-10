# 41 — Accrual ledger from canonical bundles

**Blocks on:** 40

## Files
- new crate under `crates/` — name and owning operator to be settled in this ticket
- `crates/telemetry-recorder/src/lib.rs` — feed

## Goal
Know what is owed, from chain state rather than telemetry about intent.

## Do
- Derive accruals from canonical included bundles, never from proposal or submission telemetry.
- Undo and re-derive across reorgs.
- Cannot collect the same fee twice across restart, backfill, or a re-reviewed proposal.

## Done when
- Restart and backfill produce the same totals.
