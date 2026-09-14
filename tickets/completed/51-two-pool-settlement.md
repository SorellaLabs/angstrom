# 51 — A real two-pool fixture, settled on Anvil

**Blocks on:** 38
**Closes:** ISSUES.md 9 (PR #680 A.9)
**Follows:** 32, 33

## Files
- `crates/types/src/traits/bundles.rs:1095-1112` — `tob_order`, hardcoding `asset_in(T1)`
- `crates/types/src/traits/bundles.rs:1116-1127` — `tob_with_gross`
- `crates/types/src/traits/bundles.rs:1324-1414` — `two_pools_sharing_token0`
- `crates/types/tests/anvil_settlement.rs:430-488` — "A pool per scenario"

## Goal
PLAN.md's "two pools sharing token0" is established on both token axes and against the contract.

## Do
1. Parameterise `tob_order` / `tob_with_gross` on the pool's token1. Pool B in
   `two_pools_sharing_token0` is `(T0, T1_B)` but its searcher says `asset_in = T1`, so
   `process_solution` books `external_swap(.., tob.asset_in, ..)` against the wrong token. Fix the
   fixture so pool B's order is `T1_B -> T0`.
2. Extend the unit test's assertions past `T0`: assert the `T1` and `T1_B` asset entries
   independently — each pool's input lands on its own token1 and nothing lands on the other's.
3. In `anvil_settlement.rs`, add a scenario that builds **one bundle spanning two pools** that
   share token0, and settles it. Assert every token delta (`T0`, `T1`, `T1_B`), the exact `save` on
   `T0` as the checked sum of the two per-pool fees, and reward growth on both pools.
4. Mutation-check the new assertions: swap the two searchers' input assets and confirm the unit test
   fails; hand the second pool the first pool's gross and confirm the anvil scenario fails.

## Done when
- `two_pools_sharing_token0`'s fixture is well-formed on every token and asserts on every token.
- A single two-pool bundle settles against unchanged Angstrom with zero unresolved deltas.
- Both mutations above fail the tests that own them.

## Notes
The `T0` axis of the existing test is correct and discriminating — ticket 33 chose grosses `1_001`
and `2_002` at `750_000` so that per-pool fees (`251 + 501`) and a single split of the aggregate
(`751`) differ by a unit, and asserted that gap is real before relying on it. That half stands.
What the reviewer caught is that the fixture is malformed on the token1 side and nothing looks
there, and that the anvil test only ever settles one pool per bundle, so "checked accumulation
across pools sharing token0" has never actually been executed on chain.

`solve` (`bundles.rs:1170`) already calls `asset_builder.order_assets_properly()` and iterates the
pools in order, so the builder-side plumbing for a two-pool bundle exists; it is the fixture and the
anvil harness that stop short.

Blocks on 38 so the new anvil scenario runs in CI rather than joining the one that never does. The
anvil test's per-scenario pool isolation ("every bundle is built from a snapshot that still matches
the chain") is worth keeping for the single-pool scenarios; the two-pool scenario needs two pools
deployed before its one submission, which the existing setup already does for its two scenarios.

**As built.** Both halves land where the ticket put them.

`tob_order` and `tob_with_gross` take the pool's token1 and the ToB `quantity_in`; pool B's
searcher in `two_pools_sharing_token0` is now `T1_B -> T0`. The token1 assertions go through the
existing `asset(token)` helper rather than a new one: each token1's entry is
`(take 0, settle quantity_in, save 0)` — the searcher's input arrives before settlement, so nothing
is borrowed from Uniswap and nothing is left to sweep. The two pools pay different `quantity_in`s
(`1_000_000` / `2_000_000`) on purpose: with equal quantities a crossed pair of searchers books the
same per-token totals as the right pair, and the mutation this ticket asks for would be invisible.
The T0 axis — grosses `1_001` / `2_002`, the `251 + 501 != 751` gap — is untouched.

`anvil_settlement.rs`: `settle` takes a slice of `(pool, gross)` and builds one bundle over all of
them — one `for_gas_finalization`, one funding pass, one submission — reporting
`Settled { encoded_save, balance_delta, pools: Vec<PoolSettled> }`. `deploy_pool` is a thin
wrapper over `deploy_tokens::<N>` + `setup_pool`; `deploy_pools_sharing_token0` deploys three
tokens, sorts them, and pairs the lowest with each of the other two at consecutive store indexes.
All four pools are deployed before the first submission, for the nonce reason ticket 32 recorded.
The scenario runs at `(750_000, 750_000)` with the unit test's grosses and asserts: receipt
success; `save` on t0 is `fee_A + fee_B` plus both books' user fees plus the swept per-pool
residuals, and is not a single split of the aggregate gross; Angstrom's t0 balance grew by `save`
plus both pools' rewards; and, per pool through `assert_pool_settled` (which the single-pool
scenarios now share), the t1 deltas and reward growth against that pool's own `RewardsUpdate`.
Measured: t0 `save` `2_750 = 251 + 501 + 999 + 999`, rewards `3_747` and `4_498`.

**The t1 deltas are not `quantity_in` and `0`; the ticket's reading was off by a unit.** The
PoolManager gains `999_999` of each pool's t1 and Angstrom keeps `1`. The book is priced at the
ToB end price after a `Ray` round-trip, so the net swap the bundle encodes
(`PoolUpdate.swap_in_quantity`) comes up one t1 short of the searcher's `quantity_in`; the bundle's
t1 `Asset` entry is `(take 0, settle 999_999, save 1)`, the unit put there by `collect_extra`. The
assertions pin what is true: `pool_manager_t1 + angstrom_t1 == quantity_in` — the searcher's t1
reaches the PoolManager or Angstrom and nowhere else, the book netting to zero — and
`angstrom_t1 == save(t1)` — Angstrom keeps exactly what the bundle said it would. Both hold on all
three scenarios; the single-pool scenarios gained them through the shared helper.

The scenario is a third leg of `builder_bundles_settle_against_unchanged_angstrom` rather than its
own `#[tokio::test]`: nextest runs tests in separate processes concurrently and every harness
spawns anvil on the fixed `/tmp/anvil.ipc`, so a second test in the file would race the first.

Mutation checks: (a) crossing the two searchers' input assets in the unit fixture fails
`two_pools_sharing_token0` on the T1 entry (`save` becomes `1_000_000`, the other pool's input);
(b) handing pool B pool A's gross in the anvil scenario's expectation fails it on the t0 `save`
(`2_750` measured against `2_500` expected). Both restored byte-exact — `cmp` against a
pre-mutation copy for (b), `git diff --stat` identical to the pre-mutation snapshot for both.

Verification: `cargo nextest run -p angstrom-types two_pools_sharing_token0` — 1 passed;
`cargo nextest run -p angstrom-types --features anvil --test anvil_settlement` against the public
fork — 1 passed, three scenarios; `cargo +nightly fmt -p angstrom-types`;
`cargo clippy -p angstrom-types --all-targets --features anvil -- -D warnings -A clippy::result_large_err -A mismatched_lifetime_syntaxes`
clean.

**Review fixes.** Each pool in a bundle now pays a distinct `quantity_in` — `TOB_QUANTITY_IN`
times its position plus one — so `PoolSettled.tob_quantity_in` is carried per pool instead of
restating the constant, and the on-chain `pool_manager_t1 + angstrom_t1 == quantity_in` check
tells pool A's t1 from pool B's, as the unit fixture already did with `1_000_000` / `2_000_000`.
The single-pool scenarios still pay `1_000_000`. `assert_pool_settled`'s doc now states the
identity it pins (t1 split between the PoolManager and Angstrom, Angstrom keeping the encoded
`save`) rather than "in full". Mutation: pinning `tob_quantity_in` back to the constant fails the
scenario on pool B (`2_000_000` measured against `1_000_000` expected); restored byte-exact, `cmp`
against the pre-mutation copy and `git diff --stat` identical. On (b) above: the mutation was on
the expectation side because `settle` re-derives each pool's gross from its input, so either
direction trips the same t0 `save` assertion. Re-verified against the public fork:
`cargo nextest run -p angstrom-types --features anvil --test anvil_settlement` — 1 passed, three
scenarios; the clippy invocation above clean; `cargo +nightly fmt -p angstrom-types --check` clean.
