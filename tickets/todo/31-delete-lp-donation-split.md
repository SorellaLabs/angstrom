# 31 — Delete LP_DONATION_SPLIT

**Blocks on:** 28

## Files
- `crates/types/primitives/src/contract_payloads/angstrom/mod.rs:25`
- `crates/types/src/traits/bundles.rs:24` — the import

## Goal
Remove the last ambient rate.

## Do
- Delete `LP_DONATION_SPLIT` from
  `crates/types/primitives/src/contract_payloads/angstrom/mod.rs:25` and its import in
  `bundles.rs`.

## Done when
- The constant does not exist anywhere in the workspace.
