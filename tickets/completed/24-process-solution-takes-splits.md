# 24 — process_solution takes DonationSplits explicitly

**Blocks on:** 23

## Files
- `crates/types/src/traits/bundles.rs` — trait decl (~:35) and impl (~:217), `from_proposal`, `for_gas_finalization`

## Goal
Remove the ambient constant from bundle construction.

## Do
- `crates/types/src/traits/bundles.rs`: add `DonationSplits` to `process_solution` on both the
  trait declaration and the impl.
- Reached by `from_proposal` and `for_gas_finalization`; both pass the round's snapshot.

## Done when
- No construction path can build a bundle without being handed the rates.

## Notes
`process_solution` takes `DonationSplits` on both the trait declaration and the impl, and
`from_proposal` and `for_gas_finalization` take it and hand it on. Their callers pass
`snapshot.splits` from the round's one `DonationSplitSnapshot` — `try_build_proposal` for
`from_proposal`, `MatchingManager::build_proposal` for `for_gas_finalization` — so the two
construction paths are driven from one read rather than two.

The impl names it `_splits` for now: the rates are not read until ticket 25 replaces the `f64`
split, and `-D warnings` rejects an unused binding. `_gas_details` on `from_proposal` already
carries the same marker. `LP_DONATION_SPLIT` is untouched here; 27 and 30 retire it.

`build_dummy_for_tob_gas` and `build_dummy_for_user_gas` stay rate-free. They are gas probes with
no donations at all, so they never reach `process_solution` — the claim is that no path that
*donates* can be driven without the rates, and that holds.
