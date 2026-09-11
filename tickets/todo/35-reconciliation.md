# 35 — Reconcile allocations after inclusion

**Blocks on:** 34

## Overview
Peer finalization runs on `PoolSolution`s, upstream of where the splits are applied, and EVM
simulation only proves a bundle settles — neither is evidence the split was right. This ticket
detects a bad allocation after it has already settled, which is the most that can be done here:
it cannot prevent or reverse settlement. For each included bundle it reads the rates in force at
the construction parent and re-runs the split arithmetic through `process_solution`'s own path,
then compares that against what the bundle actually encoded. Residuals reconcile as their own
buckets, so a `save` exceeding the configured fee by exactly its residual is correct and one
exceeding it by anything else is not. A mismatch, or reconstruction data that cannot be
resolved, withholds those amounts rather than blocking anything — an unverifiable amount is not
a verified one. The real risk is step 1:
reconstruction needs pool state as of the construction parent, and if an archive node cannot
supply it, the narrowed scope gets recorded here rather
than quietly shipped.

## Files
- `crates/types/src/fee_reconciliation.rs` (new module) — the reconciliation
- `crates/types/src/fee_ledger.rs` — the ledger module from ticket 34
- `crates/types/src/traits/bundles.rs` — `process_solution`, the path to reconstruct against
- `crates/types/primitives/src/contract_payloads/protocol_fees.rs:load_from_chain` — historical rates
- `contracts/src/periphery/ControllerV1.sol:224` — `distributeFees`, unchanged

## Goal
Detect a bad allocation that already settled.

## Do

1. **Reconstruct.** For each included bundle, take the construction parent to be the canonical
   parent of the block it landed in, read the rates in force there with `load_from_chain`, and
   re-run the split arithmetic. Reuse `process_solution`'s own path rather than reimplementing
   it — a second implementation drifts, and a drift here reads as a false mismatch.

2. **Compare** against what the bundle actually encoded: per pool, the `RewardsUpdate` totals
   against the expected LP allocation, and the `Asset.save` against the expected protocol fee plus
   the swept residuals.

3. **Residuals are expected, not mismatches.** Reconcile `rounding` and `unplaced` (ticket 29) as
   their own buckets. A bundle whose `save` exceeds the configured fee by exactly its residual is
   correct; one that exceeds it by anything else is not.

4. **Withhold rather than block.** A mismatch, or a bundle whose construction parent or rates
   cannot be resolved, marks those amounts withheld from any proposed distribution and reports
   why. Missing reconstruction data is withheld too — an unverifiable amount is not a verified one.

5. **Backing check on a proposed withdrawal.** Prove the withdrawal leaves LP rewards and user
   balances backed. Angstrom's ERC20 balance is **not** the withdrawable amount — it also holds
   user funds in flight and unclaimed LP rewards. Compute withdrawable as finalized accruals minus
   withheld, and assert it against the balance as an upper bound, never as the source.

## Done when
- A deliberately mis-split bundle is flagged and withheld.
- A bundle with a legitimate residual reconciles clean.
- A bundle with no resolvable construction parent is withheld, not passed.

## Notes
Detects after inclusion; cannot prevent or reverse settlement. A successful simulation and a
passing peer-finalization result are not evidence — peer checks run on `PoolSolution`s, upstream of
where splits are applied. Required before a nonzero ToB share (rollout step 5), not before step 4.

Step 1 is the ticket's real risk: `process_solution` takes a `BaselinePoolState` snapshot, so
reconstruction needs the pool state as of the construction parent, not today's. If that turns out
not to be reachable from an archive node, reconciliation narrows to the parts that do not need pool
state — the splits themselves and `Asset.save` — and that reduction should be recorded here rather
than quietly shipped.

**The construction parent is inferred, not recorded.** Per-bundle telemetry was dropped, so
nothing records the parent a bundle was actually built on. Step 1 assumes it was the canonical
parent of the block the bundle landed in, which holds unless the bundle landed later than the
block it targeted or on a replacement branch — PLAN.md records both as accepted limitations and
asks for the construction parent to be recorded precisely because of them.

When the assumption does not hold, reconstruction reads the wrong rates and the bundle is
withheld. That is the safe direction, but it is a false positive: a rate change landing near an
inclusion boundary sends legitimate amounts to human review rather than reconciling clean. The
cost is bounded by how rarely the rates move, which is a governance action. What is lost
outright is any check on a bundle built from a stale round — with no recorded parent there is
nothing for the inferred one to disagree with.

Step 5 is why ticket 32's balance assertion is explicitly not a template: it works there only
because the Anvil harness controls the starting state.

**As built.** `crates/types/src/fee_reconciliation.rs`, beside ticket 34's ledger and beside
`bundles.rs`, whose `process_solution` is what it reconciles against. Steps 2-5 landed. Step 1
narrowed, and the narrowing runs deeper than this ticket anticipated.

**Step 1 cannot be built as written, and the reason is not the one the Notes expected.** The
ticket foresaw pool state being unreachable from an archive node. The harder problem is that
`process_solution`'s *inputs* are not recorded anywhere: it takes a `PoolSolution` and a
`BaselinePoolState`, and an included bundle encodes neither. `solution.reward_t0` is a
matching-engine output that is never encoded at all, and `orders_by_pool` wants
`OrderWithStorageData` — validation-derived state a bundle does not carry. So "reuse
`process_solution`'s own path" has nothing to feed it, archive node or not. This is the reduction
the ticket asked be recorded here rather than quietly shipped.

**What that leaves is more than the fallback the ticket described, and less than step 1.** The
Notes offered "the splits themselves and `Asset.save`" as the floor. The user-fee split turns out
to reconstruct *exactly*, with no pool state: `get_quantities_at_price` takes only the fill
amount, the gas, the pool's bundle fee and the UCP; the bundle encodes the fill
(`OrderQuantities`), the gas (`extra_fee_asset0`) and the UCP (`Pair.price_1over0`), and the
bundle fee is `AngPoolConfigEntry.fee_in_e6` in the pool config store at the construction parent.
`user_fees_for_pair` re-derives `total_user_fees` from those four, `split_user` at the parent's
rates turns it into the expected protocol fee, and
`the_user_fee_is_re_derived_from_the_encoded_orders` pins it to a closed form so the check cannot
drift into agreeing with the implementation it is checking.

**The ToB split is what is actually lost.** Its gross is `calc_vec_and_reward`'s output against
pool state at the construction parent, which only a live `EnhancedUniswapPool` produces today. So
when the rates at the parent retain a ToB share, the bundle's amounts are **withheld** rather
than compared —`a_nonzero_tob_share_withholds_rather_than_passing` asserts that, and
`DonationSplits::retains_tob()` is the one-line accessor it turns on. At the deployed
`tobLpShareE6 = 1_000_000` nothing is withheld on that ground, so rollout step 4 reconciles in
full. **Closing this is a precondition for step 5**, and it is the same work as reaching
`BaselinePoolState` at an arbitrary historical parent.

**Step 2 is half of what it asks.** `Asset.save` is compared against the expected protocol fee
plus the ToB gas the bundle encodes. The `RewardsUpdate` totals are carried on
`PoolExpectation::lp_reward` but not compared, because the expected LP allocation is
`reward_t0 + split_user_lp(fees) + split_tob_lp(gross)` and two of those three are the
unreconstructible terms above. Comparing it against the one term that is known would flag every
correct bundle.

Step 3 holds: `expected_retained` is the configured fee plus ToB gas, and `save` above it is
`residual` — its own bucket, reported rather than treated as a mismatch. `save` *below* it is a
`Mismatch` carrying the shortfall. That is an asymmetric detector and worth stating: a builder
that under-retains is caught, and one that over-retains is indistinguishable from a large
allocator residual without the residual being independently derivable. The direction it catches
is the one where the protocol is shorted.

Step 4: every non-`Reconciled` outcome is withheld, including `UnresolvedParent` — reconcile_bundle
returns a withheld result rather than an error, so an unreadable parent withholds the amount and
reports why instead of failing the run. Step 5: `withdrawable` computes finalized accruals minus
withheld and errors if that exceeds the balance, so the balance is an upper bound and never the
source.

The construction parent is inferred, as the ticket's Notes describe; `BundleReconciliation::parent`
carries that caveat at the point of use.

Coverage, each checked against the mutation that should break it: `a_legitimate_residual_reconciles_clean`,
`a_mis_split_bundle_is_flagged_and_withheld` (fails when the shortfall branch is removed),
`a_bundle_with_no_resolvable_construction_parent_is_withheld`,
`a_nonzero_tob_share_withholds_rather_than_passing` (fails when the `retains_tob` branch is
removed), `the_user_fee_is_re_derived_from_the_encoded_orders`, and
`withdrawable_is_bounded_by_the_balance_and_never_sourced_from_it` (fails when the balance bound
is dropped). Each mutation failed only the test that owns it.

`reconcile_bundle` is library API an operator's tooling calls with a provider; it is deliberately
not wired into the node, because nothing in the node may act on its result — every distribution
stays an operator-reviewed timelock execution.
