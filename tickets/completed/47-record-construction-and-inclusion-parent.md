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

**As built.** Two telemetry records, keyed by the bundle's order hashes.
`TelemetryMessage::BundleSubmitted { blocknum, construction_parent: BlockNumHash, order_hashes }`
is emitted from the submission future once `submit_tx` returns `Ok` for a bundle — `blocknum` is
the target block, the parent is the `handles.block_height` ticket 48 threads in, and the hashes are
`bundle.get_order_hashes(target_block)` taken before the bundle moves into `submit_tx`.
`BundleIncluded { blocknum, inclusion_parent, order_hashes }` is emitted by the eth manager from
`handle_commit` and `handle_reorg` through `record_included_bundle`, at the `fetch_filled_order`
seam, with the tip's number and the tip's parent hash; `ChainExt` gained `tip_parent_hash()`
(`Chain`: `tip().header().parent_hash`; the test `MockChain` a `parent_hash` field). A bundle that
landed in a block whose parent is not the construction parent is the pair of records whose
`order_hashes` match and whose parents do not — from the recorded data alone, no log
reconstruction. Matching parents produce two records and nothing else; nothing here rejects,
retries or holds a submission.

**Two records, not one carrying both parents.** The two facts are known by different tasks —
consensus at submission, the eth manager at inclusion — and a node-side join would add shared
state across them for no operational gain over joining on the key. When a bundle lands in its
target block both records carry the same `blocknum`, so they sit in the same block log.

Plumbing: `telemetry-recorder` takes `alloy` back as a dependency (ticket 18 dropped
`alloy-primitives` when nothing used it), constructors `bundle_submitted` / `bundle_included` stamp
the timestamp, `try_get_timestamp` knows both variants, and `impl OrderTelemetryExt for
TelemetryMessage` lets `telemetry_event!(message)` send a ready-made message. `crates/telemetry`
files both into the block's events; replay's exhaustive match (`_ => todo!()`) gets an explicit
no-op arm, since a record is not a replay input.

Inherited scope: `fetch_filled_order` looks at tip transactions only, so a bundle in a non-tip block
of a multi-block notification is not recorded as landed — it is not reported as filled today
either. Submission hashes use `target_block` and inclusion hashes the landing block; they differ
only for flash orders, which cannot execute outside their block.

Coverage: `a_landed_bundle_is_recorded_with_its_inclusion_parent` (`crates/eth/src/manager.rs`)
installs the telemetry sink, commits a tip carrying a real `execute` bundle, and asserts the record
names the tip, the tip's parent and the bundle's order hashes (`test_fetch_filled_orders` now shares
the `bundle_transaction` fixture). The submission-side record has no unit test: reaching it needs a
bundle `from_proposal` accepts, which needs a swap, which the consensus fixtures cannot build; it is
verified by reading and by the compiler.

Verification: `cargo nextest run -p angstrom-eth --lib` — 31 passed (this includes ticket 42's
`test_pool_config_edge_cases`, which was failing on the untouched tree and is fixed under that
ticket's Review fixes); `cargo check` across `consensus`, `angstrom-eth`, `telemetry` and
`testing-tools` with tests; clippy and fmt as recorded on ticket 44. **Mutation:** the
`record_included_bundle` call in `handle_commit` removed —
`a_landed_bundle_is_recorded_with_its_inclusion_parent` fails. Restored; `manager.rs`
byte-identical to its pre-mutation copy.

**Review fixes** (independent review of the As-built, 2026-09-15). Two real gaps and one addition.
(1) `BundleSubmitted` was emitted after `submit_tx` returned, inside the spawned task, so a reset
that aborted the task while endpoint sends were still in flight lost the record for a bundle that
had already left the node — exactly the bundle the record exists for. It is now emitted
synchronously in `try_build_proposal` before the task is spawned: the record means "built for H
and handed to submission", and one for a bundle cancelled before signing pairs with nothing.
(2) `handle_reorg` collected the landed hashes into a `HashSet` before recording, so its key was in
arbitrary order while the commit path's was in bundle order; it now records the iterator order and
builds the set from it. (3) `BundleIncluded` carries `inclusion_block`, the landing block's own
hash, so two records at one height after a same-height reorg stay distinguishable. The eth test
drives both paths now and asserts the block hash and that the key reads identically on both.
Rerun: `cargo nextest run -p angstrom-eth --lib` — 31 passed.
