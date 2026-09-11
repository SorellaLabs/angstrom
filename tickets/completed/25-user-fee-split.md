# 25 — Integer user-fee split

**Blocks on:** 24

## Files
- `crates/types/src/traits/bundles.rs:25` — the `LP_DONATION_SPLIT` import
- `crates/types/src/traits/bundles.rs:234` — the `_splits` binding on the `process_solution` impl
- `crates/types/src/traits/bundles.rs:413` — the `f64` split

## Goal
Replace the `f64` split.

## Do

1. Rename the impl's parameter `_splits` to `splits` at `bundles.rs:234`. The trait declaration
   already names it `splits`; only the impl carries ticket 24's underscore.

2. Replace both lines at `bundles.rs:413`:

```rust
// was
let total_lp_user_donate = (total_user_fees as f64 * LP_DONATION_SPLIT) as u128;
let save_amount = total_user_fees - total_lp_user_donate;

// now
let (total_lp_user_donate, save_amount) = splits.split_user(total_user_fees);
```

3. Drop the `contract_payloads::angstrom::LP_DONATION_SPLIT` import at `bundles.rs:25`. Step 2
   removes the constant's last use, so leaving the import fails `-D warnings`.

4. Nothing else moves. Both binding names are unchanged, so the book donation at `:429`
   (`solution.reward_t0 + total_lp_user_donate`), the `total_donation` fallback at `:443`, and the
   three `save_amount` settlement calls at `:512-514` all keep their current expressions.

## Done when
- `just check` is clean, and `LP_DONATION_SPLIT` has one definition and no uses.
- Bundles at 75% match the old ones except for the documented unit-level rounding change.

## Notes
`split_user` rounds LP down and gives the protocol the exact remainder, so `lp + protocol ==
gross` holds by construction — which the `f64` path did not guarantee. The two agree exactly while
`total_user_fees` stays under roughly `2^53 / 3`; above that the `f64` product loses precision and
`save_amount` can come out a unit apart. That is the documented activation difference, not a
regression — replay before **A** keeps the old path (39).

The arithmetic itself is already covered by ticket 3's tests. Conservation at the
`process_solution` level is ticket 30's; this ticket is the wiring.

Ticket 28 deletes the constant itself. The import is already gone by then, so 28 is left with the
definition only.

**As built.** All four steps landed as written; no deviations. `LP_DONATION_SPLIT` still exists at
`contract_payloads/angstrom/mod.rs:25` with zero uses, which is 28's remaining job.

`crates/types/tests/angstrom.rs::build_bundle` is the only test that drives `process_solution`, and
it is `#[ignore]`d because its base64 fixture predates `cancel_requested` on `PoolSolution` — it
fails identically with and without this change, on the `serde_json` decode at `:41`, before
`process_solution` is reached. Not revived here: ticket 32 replaces fixture-based coverage with
builder-produced bundles against real contracts. The split arithmetic itself is covered by ticket
3's `splits_conserve_and_round_lp_down`.
