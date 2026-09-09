# 17 — No config means no bundle

**Blocks on:** 16

## Files
- `bin/angstrom/src/components.rs` — init failure
- `crates/consensus/src/rounds/mod.rs:277` — `matching_engine_output`

## Goal
Never build on a guessed or defaulted rate.

## Do
- A failed init load is fatal: the node does not start rather than starting without config.
- The zero address or a zero deployed block (ticket 12 defaults) means no config: do not build
  affected bundles.
- A round with no rates produces no proposal. No fallback to a default or a previous value.

## Done when
- With the init load forced to fail, the node does not start.
- With the config address unset, no bundle is built and no default rate is used.
