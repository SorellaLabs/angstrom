# 18 — Carry the config in the eth snapshot

**Blocks on:** 14, 15

## Files
- `crates/eth/src/telemetry.rs` — `EthUpdaterSnapshot` and its `From`; delete
  `ProtocolFeeConfigChange` and its tests
- `crates/telemetry-recorder/src/lib.rs` — delete the `ProtocolFeeConfigChange` variant,
  `ProtocolFeeConfigChangeCause`, and the `try_get_timestamp` arm
- `crates/telemetry-recorder/Cargo.toml` — drop `alloy-primitives`, added only for the variant's
  `B256`
- `crates/telemetry/src/lib.rs:210` — drop the match arm routing the deleted variant
- `crates/eth/src/manager.rs` — the import, the two `telemetry_event!` sites, and the
  `next_config_change` test helper with the tests that use it
- `crates/types/primitives/src/contract_payloads/protocol_fees.rs` — serde for the split types

## Goal
The config the node is running on is visible per notification, from the eth snapshot alone. One
telemetry surface, not two.

## Do
- Add `protocol_fee_config: DonationSplitSnapshot` to `EthUpdaterSnapshot`, beside `pool_store`
  and `node_set`, filled from the cleanser field in the `From` impl.
- Delete `ProtocolFeeConfigChange`, `TelemetryMessage::ProtocolFeeConfigChange`, and
  `ProtocolFeeConfigChangeCause` outright, with every call site listed above. No per-change
  telemetry record remains.
- Move the `telemetry_event!(EthUpdaterSnapshot::…)` call in `on_canon_update` to **after** the
  `handle_reorg` / `handle_commit` match, so the snapshot carries the config in force as of the
  notification's tip rather than the pair it held before. See the note below — this moves every
  other field on the snapshot too.
- `EthUpdaterSnapshot` is `Serialize, Deserialize` and round-trips through `serde_json`, so both
  split types need the same. `DonationSplitSnapshot` takes a plain derive — its fields are
  already public and its `splits` field inherits the check below.
- `DonationSplits` must **not** take a plain derive: a derived `Deserialize` is a second
  constructor that skips the `DENOM` bounds check, and `new` is documented as the only one. Keep
  the plain `Serialize` and route `Deserialize` through `new` — `#[serde(try_from = ...)]` over a
  raw pair — so the invariant holds on the way in.
- Drop `user_lp_share_e6()` / `tob_lp_share_e6()` if the deletions leave them with no callers.

## Done when
- `TelemetryMessage` has no `ProtocolFeeConfigChange` variant and the workspace builds.
- Each `EthSnapshot` carries the splits in force at that notification's tip, applied logs
  included.
- A share above `DENOM` fails to deserialize instead of producing a `DonationSplits`.
- A notification that changed the config, and one that did not, are both distinguishable by
  comparing consecutive snapshots.

## Notes
Change history is now derived, not recorded: an operator diffs `protocol_fee_config` across
consecutive `EthSnapshot`s. `EthUpdaterSnapshot` already carries `chain_update`
(`New` vs `Reorg { new, old }`), so a diff that came from a reorg inversion is still
distinguishable from a governance action — by the notification's shape rather than by a `cause`
field.

Two things the snapshot cannot represent, accepted as the cost of one surface: a notification
carrying two setters keeps only the final pair, and a reorg that inverts one change and applies
another shows only the replacement.

Moving the emission point is what makes the field mean "active". It also changes
`angstrom_tokens`, `pool_store` and `node_set` from pre- to post-notification state. That is a
behavior change to an existing telemetry surface — intended here, since every one of those fields
describes state the notification just updated, but call it out in the PR. Leaving the call where
it is instead would make `protocol_fee_config` lag by one notification and attribute changes to
the wrong block.
