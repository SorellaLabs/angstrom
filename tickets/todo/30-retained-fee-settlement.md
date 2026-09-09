# 30 — Settle both retained portions through save

**Blocks on:** 29

## Files
- `crates/types/src/traits/bundles.rs`
- `crates/types/primitives/src/contract_payloads/asset/state.rs` — `AssetBuilder`, `add_gas_fee`, `collect_extra`

## Goal
Land the protocol's share in `save` with no contract change.

## Do
- `save_amount = user_protocol_fee.checked_add(tob_protocol_fee)`, erroring on overflow.
- Reuse the existing retained-fee pattern:
  `allocate(Reward, t0, total_donation)`, `allocate(Reward, t0, save_amount)`,
  `add_gas_fee(Reward, t0, save_amount)`.
- `total_donation` comes from the actual merged donations.

## Done when
- Exact `save` on chain, zero unresolved deltas.
- The saved amount is both allocated and reserved, so `collect_extra` cannot double count it.

## Notes
`add_gas_fee` increments `save` despite its name. `tribute` moves `take`, not `save`, and is not a
substitute. Gas accounting stays at its own call sites.
