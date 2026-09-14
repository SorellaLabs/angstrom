# 47 — Record the construction parent and the inclusion parent

**Blocks on:** 48
**Closes:** ISSUES.md 6, the "record" half
**Follows:** 21, 46

## Files
- `crates/consensus/src/rounds/proposal.rs:208-320` — the submission future, where the construction
  parent is in scope
- `crates/types/src/submission/mod.rs:186-230` — `submit_tx`, which ticket 48 teaches the parent
- `crates/eth/src/manager.rs:322-345` — `fetch_filled_order`, which already recognises a landed bundle
- `crates/eth/src/telemetry.rs` — where an operator-facing record belongs

## Goal
When a bundle lands on a parent other than the one it was built for, that is visible afterwards.

## Do
- Record the construction parent (`SharedRoundState::block_height`) with every submission, using
  the threading ticket 48 adds, so each submitted bundle is attributable to the state it was priced
  at.
- When a bundle is observed landing on chain, record the parent of the block it landed in.
  `fetch_filled_order` already decodes Angstrom bundles out of canonical blocks, so the inclusion
  side has a seam.
- Make a mismatch legible without reconstructing it from logs — a telemetry record, keyed by the
  bundle's order hashes, carrying both parents.

## Done when
- A bundle built for parent H and included on a block whose parent is not H is identifiable from
  the recorded data alone.
- Matching construction and inclusion parents produce no alarm.
- Nothing here rejects, retries, or holds a submission.

## Notes
PLAN.md's **Accepted limitation** is explicit that this cannot be prevented: "a transaction already
sent cannot be recalled, and mempool submissions carry no parent-hash condition. One may execute on
a replacement branch, or land in a later block if its orders are still valid." What it asks for in
exchange is visibility — "Record the construction parent and the actual inclusion parent so the
mismatch is visible afterwards" — and it closes with "Do not claim stale bundles are excluded."

So this is observability, not enforcement. Ticket 44 handles the sends that had not yet happened at
reset; this ticket handles the ones that had.

It is also a prerequisite-shaped piece for the step-5 accounting component, which must "derive
accruals from canonical included bundles, not proposal or submission telemetry" and "reconstruct
the expected allocations from each bundle's construction parent and the rates in force there". The
construction parent is exactly what that reconstruction keys on; recording it now saves the
follow-up from re-deriving it, and it is the one datum that becomes unrecoverable if it is not
captured at the time.

Blocks on 48 only because 48 is what gets the parent into `submit_tx` in the first place; the
inclusion side can be built independently.
