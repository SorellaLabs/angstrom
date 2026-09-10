# 15 — Invert config changes on reorg

**Blocks on:** 14

## Files
- `crates/eth/src/manager.rs:150` — `handle_reorg`, including the `todo!()` at `:157`

## Goal
Undo a config change that was reorged out, without reading storage.

## Do
- Scan the **old** chain's blocks for `LpDonationSplitsSet`. If any are found, restore the
  `oldUserLpShareE6` / `oldTobLpShareE6` pair from the **earliest** one — that is the state before
  the reorged-out range.
- Then apply the new chain's logs as normal (ticket 14), so a reorg that replaces one change with
  another lands on the new value.
- Delete the `todo!()`.
- Publish `EthEvent::ProtocolFeeConfigUpdated` with the new tip's identity.

## Done when
- A reorg that removes a setter transaction restores the previous rates.
- A reorg that replaces one setter with another ends on the replacement.
- No panic path remains in reorg handling.

## Notes
This is exactly why the event carries old and new values: a full-pair write with a complete
before/after record is invertible from the log alone. `apply_periphery_logs` does not do this for
nodes or pools today — do not copy that.
