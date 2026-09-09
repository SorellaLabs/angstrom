# 18 — Index LpDonationSplitsSet as telemetry only

**Blocks on:** 14

## Files
- `crates/eth/src/manager.rs` — log filter
- `crates/eth/src/telemetry.rs`

## Goal
Keep operator-facing change history without letting it configure anything.

## Do
- Keep the log filter on the config address, but as a view over what storage already decided.
- Process every relevant block in a notification, not only the tip, and account for removed blocks.
- Never feed it into the snapshot consumers use.

## Done when
- Change history is reported.
- Deleting the indexer changes no bundle.
