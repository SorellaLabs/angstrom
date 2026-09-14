# 42 — Contain the two out-of-plan behaviour changes in the eth manager

**Blocks on:** —
**Closes:** ISSUES.md 6
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
