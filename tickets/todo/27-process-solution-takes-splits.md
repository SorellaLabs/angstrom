# 27 — process_solution takes DonationSplits explicitly

**Blocks on:** 24

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
