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
