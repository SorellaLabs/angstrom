# 34 — Accrual ledger from canonical bundles

**Blocks on:** 27

## Overview
A new module in `crates/eth` answering what the protocol is owed, derived from chain state
rather than from telemetry about what the builder meant to do. It accrues from bundles decoded
out of included blocks' calldata on the same canonical commit/reorg feed the eth cleanser uses,
processing every block in a notification rather than just the tip — `apply_periphery_logs` had
exactly that bug. Accruals key on `(block_hash, tx_hash, pool_id, token)`, because block number
is not a key when same-height reorgs can give two different bundles the same one. That key is
also what makes a reorg a delete rather than an inversion, and what makes restart and backfill
idempotent by upsert. Accrual happens on commit so the ledger stays current, but rows only
become withdrawable at finalization, so a reorg can never remove a row that was already
withdrawable. Nothing here holds withdrawal authority; the module produces numbers a human reads
before an operator-reviewed timelock execution.
## Files
- `crates/eth/src/fee_ledger.rs` (new module) — the ledger; owning operator settled in this ticket
- `crates/eth/src/manager.rs:320` — `fetch_filled_order`, the existing bundle decode to reuse
- `crates/eth/src/manager.rs:402` — `logs_in_block_order`, the every-block pattern to follow
- `crates/eth/src/manager.rs:150,209` — `handle_reorg` / `apply_periphery_logs`, the commit/reorg feed

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
The owning operator is a deliverable of this ticket, not a precondition — PLAN.md requires it
named in the rollout artifacts before a nonzero ToB share.

**It lives in `crates/eth`, not a new crate.** Everything step 1 asks for is already there:
`EthDataCleanser` subscribes to the canonical feed, `fetch_filled_order` (`manager.rs:320`)
already filters transactions to the Angstrom address and decodes `executeCall` calldata
through `AngstromBundle::pade_decode`, and `handle_reorg` already diffs old against new. A
separate crate would depend on `eth` for all of it and duplicate the decode.

One thing to fix rather than copy: `fetch_filled_order` reads
`successful_tip_transactions()` — the tip block only. The ledger must walk every block in a
notification, the same bug ticket 14 fixed for logs. Follow `logs_in_block_order`
(`manager.rs:402`), which iterates `block_hashes()`, rather than the tip-only helper.

There is no per-bundle telemetry to accrue from, and there would be no point if there were:
accruing from what the builder recorded would mean the ledger agrees with it by construction,
and ticket 35's whole purpose is to detect a builder that got it wrong. Calldata is the source.

Nothing here holds withdrawal authority or initiates a distribution. `distributeFees` stays
owner-only and operator-reviewed; this module produces numbers a human reads.
