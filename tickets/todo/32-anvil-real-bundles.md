# 32 — Builder bundles against unchanged Angstrom

**Blocks on:** 27

## Files
- `testing-tools/src/contracts/environment/angstrom.rs` — `AngstromEnv`, real deployment
- `testing-tools/src/providers/anvil_submission.rs:27` — `submit`, bundle → signed tx
- `testing-tools/src/types/initial_state.rs` — pool and balance setup
- `crates/types/src/traits/bundles.rs` — code under test

## Goal
Prove settlement against real contracts, not fixtures.

## Do

1. Stand up `AngstromEnv` with a funded pool and orders, drive `process_solution` /
   `from_proposal` to produce a real `AngstromBundle`, and submit it through
   `AnvilSubmissionProvider`. The harness already deploys unchanged Angstrom and applies the
   storage overrides `fetch_needed_overrides` asks for — do not hand-assemble a bundle.

2. **Zero unresolved deltas is the tx succeeding.** `Settlement._saveAndSettle`
   (`contracts/src/modules/Settlement.sol:82`) computes `bundleDeltas.sub(addr, saving + settle)`
   and reverts `BundlDeltaUnresolved(addr)` on any nonzero. Assert the receipt succeeded and say in
   the test *why* that is the delta assertion — a later reader should not re-derive it.

3. **Exact `save` needs two assertions, because there is no on-chain counter.** `pullFee`
   (`TopLevelAuth.sol:180`) transfers from the raw ERC20 balance; nothing accumulates `save` in
   storage. So:
   - decode the submitted bundle's `Asset` array and assert `save` for t0 equals
     `user_protocol_fee + tob_protocol_fee` plus the residuals `collect_extra` swept;
   - assert the Angstrom contract's t0 ERC20 balance grew by exactly that amount across the tx.

4. **Reward growth** — read `poolRewards.globalGrowth` and the per-tick growth
   (`contracts/src/modules/PoolUpdates.sol:61-91`) before and after, and assert the increase matches
   the bundle's `RewardsUpdate`.

5. Run at `(750_000, 1_000_000)` and at a nonzero ToB share, so the ToB fee path is exercised
   somewhere before rollout step 5 turns it on for real.

## Done when
- Hand-written fixtures are not what proves this.
- Exact `save`, a successful settlement, and expected reward growth all assert against a bundle the
  builder produced.

## Notes
This is the ticket that would catch a `save_amount` that is right in the builder's arithmetic but
wrong against the contract — the thing no unit test in `crates/types` can reach, and the reason
PLAN.md calls out fixtures explicitly.

Step 3's balance check is only sound because the harness controls the starting state. It is **not**
a template for ticket 38, where Angstrom's balance also holds user funds in flight and is
explicitly not the withdrawable amount.

`crates/types/tests/angstrom.rs::build_bundle` is the existing fixture test and is `#[ignore]`d with
a stale base64 blob (its `PoolSolution` predates `cancel_requested`). Deleting it once this ticket
lands is reasonable; it is not carrying coverage today.
