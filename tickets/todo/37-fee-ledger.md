# 37 — Accrual ledger from canonical bundles

**Blocks on:** 36

## Files
- `crates/fee-ledger/` (new) — name and owning operator to be settled in this ticket
- `crates/eth/src/manager.rs:150,209` — `handle_reorg` / `apply_periphery_logs`, the commit/reorg feed
- `crates/telemetry-recorder/src/lib.rs` — ticket 36's `BundleFees`, reconstruction input only

## Goal
Know what is owed, from chain state rather than telemetry about intent.

## Do

1. **Source is canonical inclusion.** Subscribe to the same canonical commit/reorg feed the eth
   cleanser uses, and accrue from bundles found in included blocks — decoded from the transaction
   calldata, not from proposal or submission telemetry. Process every block in a notification, not
   just the tip; `apply_periphery_logs` had exactly that bug (ticket 14).

2. **Key every accrual by `(block_hash, tx_hash, pool_id, token)`.** Block *number* is not a key —
   same-height reorgs give two different bundles the same number. This key is also what makes
   step 4 hold.

3. **Reorgs undo and re-derive.** On a reorg, delete every accrual whose `block_hash` is in the
   old chain, then apply the new chain's blocks. Storage keyed by hash makes this a delete rather
   than an inversion — do not try to invert amounts.

4. **Idempotent by construction.** Writes are upserts on the step-2 key, so a restart that
   re-processes a block, a backfill that overlaps, and a re-reviewed proposal all converge on the
   same row instead of adding a second one. Persist a watermark (last finalized block processed)
   so a restart knows where to resume, but correctness must not depend on the watermark being
   right.

5. **Finalization gates withdrawal, not accrual.** Accrue on commit so the ledger is current;
   mark rows withdrawable only at `EthEvent::FinalizedBlock`. A reorg can only ever remove a row
   that was not yet withdrawable.

## Done when
- Restart and backfill produce the same totals.
- A reorg that removes an included bundle removes its accrual.
- The same fee cannot be collected twice across restart, backfill, or a re-reviewed proposal.

## Notes
Crate name and owning operator are a deliverable of this ticket, not a precondition — PLAN.md
requires both named in the rollout artifacts before a nonzero ToB share.

Ticket 36's `BundleFees` is the *reconstruction input* for ticket 38, never the accrual source.
Accruing from it would mean the ledger agrees with the builder by construction, and 38's whole
purpose is to detect a builder that got it wrong.

Nothing here holds withdrawal authority or initiates a distribution. `distributeFees` stays
owner-only and operator-reviewed; this crate produces numbers a human reads.
