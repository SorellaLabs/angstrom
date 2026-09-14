# 42 — Contain the two out-of-plan behaviour changes in the eth manager

**Blocks on:** —
**Closes:** ISSUES.md 12
**Follows:** 14, 18

## Overview
PLAN.md's node-changes table gives `crates/eth/src/manager.rs` one job: "Refresh on canonical commit
and reorg before releasing the block update." Tickets 14 and 18 did that, and moved two existing
behaviours with it — the periphery log scan widened from tip-only to every block, and the telemetry
snapshot now describes post-notification rather than pre-notification state. Both are defensible and
both were needed. Neither is in the plan, and neither has been contained: the widened scan exposes
non-idempotent state, and the telemetry change silently alters an operator-facing surface. This
ticket makes the first safe and the second visible.

## Files
- `crates/eth/src/manager.rs:220` — `apply_periphery_logs`, now walking every block
- `crates/eth/src/manager.rs:296-302` — `pool_store.new_pool` and the `angstrom_tokens` counters
- `crates/eth/src/manager.rs:402` — `logs_in_block_order`
- `crates/eth/src/manager.rs:141-158` — `on_canon_update` and the moved `telemetry_event!`
- `crates/eth/src/telemetry.rs` — `EthUpdaterSnapshot`

## Goal
Applying the same log twice cannot corrupt state, and the telemetry change is documented where its
consumers will see it.

## Do
1. **Make pool and node application idempotent.** `PoolConfigured` calls
   `self.pool_store.new_pool(..)` and does `*self.angstrom_tokens.entry(asset).or_default() += 1`
   for both assets. Neither checks whether the pool is already known, so applying the same
   `PoolConfigured` twice adds a duplicate store entry and double-counts both token refcounts.
   Make re-application a no-op.
2. **Record the telemetry semantics change** in the PR description and wherever `EthUpdaterSnapshot`
   is documented for operators. Ticket 18 asked for exactly this and it has not been done.
3. Confirm no consumer of `EthSnapshot` compares `angstrom_tokens` / `pool_store` / `node_set`
   across the deploy boundary in a way the shift breaks.

## Done when
- Applying a notification whose blocks were already applied leaves `pool_store` and
  `angstrom_tokens` unchanged.
- The telemetry semantics change is written down somewhere an operator reading `EthSnapshot` will
  find it.

## Notes
**The widened scan is a net fix, not a regression.** `main` scanned only
`receipts_by_block_hash(chain.tip_hash())`, so `NodeAdded` / `NodeRemoved` / `PoolConfigured` /
`PoolRemoved` in any non-tip block of a multi-block notification were silently dropped. Ticket 14
had to walk every block for the config, and node and pool logs came along. That is strictly better.

The exposure runs the other way. Reth's `Commit { new }` and `Reorg { old, new }` chains carry blocks
that have not been applied yet, so a double-apply is not expected in normal operation — but `main`'s
tip-only scan made the question moot and the widened scan does not. Step 1 is cheap insurance on
state that has no idempotency of its own, on a path that now touches many more logs than it used to.

**The telemetry move was required and its side effect was not.** Ticket 18 moved
`telemetry_event!(EthUpdaterSnapshot::…)` from before the commit/reorg match to after it, so
`protocol_fee_config` means "in force at this tip" rather than lagging by one notification. That is
correct and the field is useless without it. But the same move flips `angstrom_tokens`, `pool_store`
and `node_set` from pre- to post-notification state on every snapshot. Ticket 18's own notes say:
"That is a behavior change to an existing telemetry surface — intended here [...] but call it out in
the PR."

Nothing about this ticket should revert either change. Both are right; they are just unguarded.

**As built.** Step 1 landed with one refinement, steps 2 and 3 as written.

- `PoolConfigured`: the controller emits the same event for a reconfiguration, which Angstrom's
  `configurePool` applies in place, so "already known" cannot mean "skip". A known pair keeps its
  `store_index` (matching the on-chain entry it modifies) and its tokens are not counted again; an
  entry identical to the stored one is a re-delivered log and `continue`s without re-announcing
  `NewPool`. `AngPoolConfigEntry` gained `PartialEq, Eq` for that comparison.
- `PoolRemoved`: the refcount lookup uses `get` instead of `entry().or_default()`, so a duplicate
  removal no longer inserts a phantom zero-count token. Observed and left alone, out of scope: the
  count is never decremented when it is above one (`main`'s behaviour).
- `NodeAdded` / `NodeRemoved` only announce when the set actually changed.
- `test_pool_config_edge_cases` was pinning the double count — it asserted both tokens still present
  after the pool's removal, which only held because two `PoolConfigured` logs had counted them twice.
  Rewritten: after the two logs the store has one entry at index 0 carrying the new tick spacing and
  each token is counted once; after the removal both tokens are gone. `test_duplicate_pool_removal`
  now also asserts `angstrom_tokens` is empty. New:
  `re_applying_a_notification_leaves_pool_and_node_state_unchanged` applies the same block twice and
  asserts the entry, the counts, the node set and the absence of any second announcement.
- Step 2: the `EthUpdaterSnapshot` doc comment now states that every field is post-notification,
  that before PR #680 `angstrom_tokens` / `pool_store` / `node_set` lagged by one notification, and
  what that means for a consumer that seeds from a snapshot. For the PR description:

  > **Telemetry semantics change.** `EthUpdaterSnapshot` is now emitted after a notification's logs
  > are applied, so `angstrom_tokens`, `pool_store` and `node_set` describe the state *at* the
  > notification's tip rather than the state before it (`protocol_fee_config` needs this to mean
  > "in force at this tip"). Snapshots recorded before this change lag those three fields by one
  > notification. Replay seeds the cleanser from a recorded snapshot and then replays that block's
  > notification, so with the new semantics it re-applies that block's periphery logs; log
  > application is now idempotent, which is what keeps that harmless.

- Step 3, confirmed: the only consumers are `crates/telemetry` (decodes it into a block log) and
  `testing-tools/src/replay/runner.rs`, which seeds `angstrom_tokens` / `pool_store` / `node_set`
  from `block_log.eth_snapshot` — recorded at tip *N*, now post-notification — and then replays
  *N*'s own notification. That is the one place the shift changes behaviour: *N*'s `PoolConfigured`
  / `NodeAdded` logs are applied a second time, and step 1 is exactly what makes that a no-op. Across
  the deploy boundary, older recorded snapshots are pre-notification and replay from them applies
  *N*'s logs once, as before; nothing compares the two generations against each other.

Verification: `cargo nextest run -p angstrom-eth --lib` — 30 passed (with ticket 37's changes in the
same tree); `cargo +nightly fmt`. **Not run, by request:** workspace tests and the mutation checks
(re-counting a known pair, restoring `entry().or_default()`); see ticket 37's note on clippy.
