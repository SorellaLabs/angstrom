# 07 — Publish config updates from the eth manager

**Blocks on:** 06

## Files
- `crates/eth/src/manager.rs` — `EthEvent`, `handle_commit`, `handle_reorg`, `get_protocol_config_update`

## Goal
Get a snapshot out of `EthDataCleanser` to consumers.

## Do
- `EthEvent::ProtocolFeeConfigUpdated(DonationSplitSnapshot)` in `crates/eth/src/manager.rs`.
- `protocol_fee_config: Option<DonationSplitSnapshot>` on the cleanser.
- Emit from `handle_commit` and `handle_reorg`.

## Done when
- Consumers receive a snapshot carrying the publishing block's identity.

## Notes
This landed reading `LpDonationSplitsSet` logs. That is the wrong source — see tickets 13-15 and 18.
`handle_reorg` still has a `todo!()`.
