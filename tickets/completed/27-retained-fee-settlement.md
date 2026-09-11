# 27 — Settle both retained portions through save

**Blocks on:** 26

## Files
- `crates/types/src/traits/bundles.rs:409` — the user split binding
- `crates/types/src/traits/bundles.rs:429` — `_tob_protocol_fee` from ticket 26
- `crates/types/src/traits/bundles.rs:512-514` — the three settlement calls
- `crates/types/primitives/src/contract_payloads/asset/state.rs` — `collect_extra`, unchanged
- `crates/types/primitives/src/contract_payloads/asset/builder.rs:103` — `get_asset_array`, unchanged

## Goal
Land the protocol's share in `save` with no contract change.

## Do

1. Rename the user half so the two fees read alike. At `:409`:

```rust
let (total_lp_user_donate, user_protocol_fee) = splits.split_user(total_user_fees);
```

2. Drop the underscore on ticket 26's `_tob_protocol_fee` at `:429`.

3. Sum them where `save_amount` used to be bound, after the ToB split so both are in scope:

```rust
let save_amount = user_protocol_fee
    .checked_add(tob_protocol_fee)
    .ok_or_else(|| eyre::eyre!("retained fees exceed u128"))?;
```

4. The three calls at `:512-514` keep their current shape — they already read `save_amount`:

```rust
asset_builder.allocate(AssetBuilderStage::Reward, t0, total_donation);
asset_builder.allocate(AssetBuilderStage::Reward, t0, save_amount);
asset_builder.add_gas_fee(AssetBuilderStage::Reward, t0, save_amount);
```

5. `total_donation` at `:443` already comes from `donation.total_donated`, the actual merged
   donations. Leave the `unwrap_or` fallback alone — see Notes.

## Done when
- Exact `save` on chain, zero unresolved deltas.
- The saved amount is both allocated and reserved, so `collect_extra` cannot double count it.
- `save_amount` carries only the configured fee. Unallocated remainders reach `save` through
  `collect_extra` as today and are never added here.

## Notes
`add_gas_fee` increments `save` despite its name. `tribute` moves `take`, not `save`, and is not a
substitute. Gas accounting stays at its own call sites.

**Why both `allocate` and `add_gas_fee`.** `collect_extra` (`asset/state.rs:169`) moves
`contract_liquid.saturating_sub(settle)` into `save`. The `allocate` call spends the fee out of
`contract_liquid` — borrowing from Uniswap if short — so it is no longer residual when
`collect_extra` runs; `add_gas_fee` then puts it into `save` directly. Drop the `allocate` and the
same amount lands in `save` twice. This is the pattern the user fee already used, unchanged.

`collect_extra` is called once, in `AssetBuilder::get_asset_array` (`asset/builder.rs:103`), after
the four stages are chained with `and_then`. That single call is where every unallocated remainder
becomes `save` — which is why ticket 29's residual must not be added to `save_amount`, or it is
counted twice.

The `unwrap_or(solution.reward_t0 + total_lp_user_donate)` fallback at `:443` is reached only when
both donation vectors are `None`, which implies no ToB order, so it cannot miss a ToB fee. Ticket
31 makes that branch's retention explicit; it is not a correctness gap here.

**As built.** All five steps landed as written. The only structural change is where `save_amount`
is bound: it moves from the user split down below the ToB split, which is the one point where both
fees are in scope. The three settlement calls and `total_donation`'s `unwrap_or` fallback are
untouched.

At the deployed `tobLpShareE6 = 1_000_000` the ToB half is always `0`, so `save_amount` is
numerically identical to what ticket 25 produced. The `checked_add` is unreachable until rollout
step 5 — it guards the configuration that makes it reachable, not today's.
