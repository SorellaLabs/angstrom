# 17 — No config means no bundle

**Blocks on:** 16

## Files
- `bin/angstrom/src/components.rs` — init failure
- `crates/consensus/src/rounds/mod.rs:277` — `matching_engine_output`

## Goal
Never build on a guessed or defaulted rate.

## Do
- A failed init load is fatal: the node does not start rather than starting without config.
- With the config address unset, a read past `PROTOCOL_FEE_CONFIG_DEPLOYED_BLOCK` fails: do not
  build affected bundles. Blocks at or before that height resolve to the baked-in const (13)
  and are not a failure.
- A round with no rates produces no proposal. No fallback to a default or a previous value.

## Done when
- With the init load forced to fail, the node does not start.
- With the config address unset, no bundle is built and no default rate is used.

## Notes
"With the config address unset" resolves as **refuse to start**, not "run without proposing":
`load_from_chain` errors on a zero address past `PROTOCOL_FEE_CONFIG_DEPLOYED_BLOCK`, and
`components.rs` propagates that out of `initialize_strom_components`, so the process exits with the
reason. `SharedRoundState.protocol_fee_config` therefore stays non-optional and
`matching_engine_output` needs no change — "a round with no rates" is unreachable by construction,
which is what makes "no fallback to a default or a previous value" hold. Neither
`DonationSplits` nor `DonationSplitSnapshot` derives `Default`, and nothing calls
`unwrap_or_default` on a snapshot, so there is no default to fall back to.

Until ticket 45 sets the real address, every network holds `Address::ZERO` / block `0`, so a node
on a live chain head will not start. That is the intended fail-closed posture, not a regression.
