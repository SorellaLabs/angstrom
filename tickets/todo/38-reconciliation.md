# 38 — Reconcile allocations after inclusion

**Blocks on:** 37

## Files
- `crates/fee-ledger/` — the crate from ticket 37
- `crates/types/src/traits/bundles.rs` — `process_solution`, reused to reconstruct
- `crates/types/primitives/src/contract_payloads/protocol_fees.rs:load_from_chain` — historical rates
- `contracts/src/periphery/ControllerV1.sol:224` — `distributeFees`, unchanged

## Goal
Detect a bad allocation that already settled.

## Do

1. **Reconstruct.** For each included bundle, take its construction parent from ticket 36's
   record, read the rates in force at that parent with `load_from_chain`, and re-run the split
   arithmetic. Reuse `process_solution`'s own path rather than reimplementing it — a second
   implementation drifts, and a drift here reads as a false mismatch.

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

Step 5 is why ticket 32's balance assertion is explicitly not a template: it works there only
because the Anvil harness controls the starting state.
