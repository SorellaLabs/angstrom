# 26 — Apply the ToB split before the donation merge

**Blocks on:** 25

## Files
- `crates/types/src/traits/bundles.rs:352-378` — `tob_swap_info` and its `Err` arm at `:370`
- `crates/types/src/traits/bundles.rs:431` — `tob_donation_vec`
- `crates/types/src/traits/tob.rs` — `calc_vec_and_reward`, unchanged
- `crates/types/src/uni_structure/pool_swap.rs` — `t0_donation_vec`, unchanged

## Goal
Give the protocol a share of ToB surplus.

## Do

1. **Split at the point of use, not inside `tob_swap_info`.** Leave `tob_swap_info` as
   `Option<(PoolSwapResult, u128)>` carrying the **gross** `reward_q`. `post_tob_price` at `:384`
   and `net_pool_vec` at `:450` both destructure it and both must keep seeing gross — that is what
   "leave the post-ToB price alone" means mechanically.

2. Replace the `tob_donation_vec` map at `:431`:

```rust
// was
let tob_donation_vec = tob_swap_info
    .as_ref()
    .map(|(tob_vec, tob_d)| tob_vec.t0_donation_vec(*tob_d));

// now
let (tob_donation_vec, _tob_protocol_fee) = tob_swap_info
    .as_ref()
    .map(|(tob_vec, gross_tob_reward)| {
        let (tob_lp_budget, protocol) = splits.split_tob(*gross_tob_reward);
        (Some(tob_vec.t0_donation_vec(tob_lp_budget)), protocol)
    })
    .unwrap_or((None, 0u128));
```

   The closure returns a tuple because the fee has to escape it to reach ticket 27's `save_amount`.
   `unwrap_or` carries the no-ToB case, so `tob_donation_vec` stays `Option<Vec<DonationType>>` for
   the donation merge at `:435` and the fee stays a plain `u128` for 27.

3. Bind the fee as `_tob_protocol_fee` for now — 27 folds it into `save_amount`, and `-D warnings`
   rejects an unused binding. Same marker ticket 24 used for `_splits`.

4. **A selected ToB order that fails to evaluate becomes an error.** The `Err` arm at `:370` logs
   and returns `None`, which silently turns a failed evaluation into both zero ToB revenue and a
   pre-ToB `post_tob_price`. Propagate instead, keeping the log:

```rust
Err(error) => {
    error!(?error, "Error in ToB swap vs AMM");
    return Err(error);
}
```

   `calc_vec_and_reward` already returns `eyre::Result`, so this needs no new import. The `else`
   branch at `:375` (no `solution.searcher`) keeps returning `None` — that is the genuine zero case.

## Done when
- No ToB order means both values are zero — `unwrap_or` yields `(None, 0)`.
- A selected ToB order that fails to evaluate is an error, not zero revenue.
- The share touches only ToB surplus. `total_user_fees`, `solution.reward_t0`, gas, and
  unlocked-swap fees are untouched by this ticket.
- `calc_vec_and_reward`, `calc_reward`, bid ranking, swap quantities and the post-ToB price are
  unchanged; ranking still runs on gross.

## Notes
Applied once per pool to that pool's gross total, structurally: `process_solution` runs per
`PoolSolution` and the split sits directly above `t0_donation_vec`, so it cannot land per tick, per
fragment, or after cross-pool aggregation. Two pools sharing token0 accumulate separately in
`asset_builder` — ticket 33 asserts that.

At the deployed `tobLpShareE6 = 1_000_000`, `tob_lp_budget == gross` and `tob_protocol_fee == 0`,
so this path is inert until rollout step 5. Step 4 activates it at zero.

The `total_donation` fallback at `:443` is reached only when both donation vectors are `None`,
which implies no ToB, so it stays correct here. Ticket 27 revisits it to compute `total_donation`
from the actual merged donations.

**As built.** Both steps landed as written; no deviations. `tob_swap_info` still carries gross, so
`post_tob_price` and `net_pool_vec` are untouched.

Step 4 widens `process_solution`'s failure surface, which is the point but worth stating: a ToB
order that fails `calc_vec_and_reward` used to yield a bundle with no ToB donation *and* a pre-ToB
`post_tob_price` — a silently mispriced book swap on top of the lost revenue. It now aborts the
solution. The early `return` drops `process_solution_span` by RAII, so the explicit `drop` at the
end of the function is unaffected.
