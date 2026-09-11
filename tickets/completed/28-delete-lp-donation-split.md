# 28 — Delete LP_DONATION_SPLIT

**Blocks on:** 25

## Files
- `crates/types/primitives/src/contract_payloads/angstrom/mod.rs:25` — the definition

## Goal
Remove the last ambient rate.

## Do

1. Delete the line at `angstrom/mod.rs:25`:

```rust
pub const LP_DONATION_SPLIT: f64 = 0.75;
```

2. Confirm nothing references it:

```bash
grep -rn "LP_DONATION_SPLIT" crates/ bin/ testing-tools/ contracts/
```

## Done when
- The constant does not exist anywhere in the workspace.
- `cargo check --workspace --all-targets` is clean.

## Notes
Ticket 25 already removed the import at `bundles.rs:25` along with the constant's last use, so this
is the definition only — a one-line delete with no call sites to chase.

It is a `pub const` in a library crate, so it produces no dead-code warning while it lingers;
nothing fails if this ticket is deferred, which is exactly why it is worth doing rather than
forgetting. Ticket 36 does **not** depend on it staying — see that ticket's Notes on whether a
legacy `f64` path is needed at all.

**As built.** One-line delete, no call sites. The grep's only remaining hit is prose:
`contracts/script/AngstromProtocolFeeConfig.s.sol:26` says the deployed `750_000` matches "the
`LP_DONATION_SPLIT` this replaces", which is a historical note rather than a reference and is left
as it is.
