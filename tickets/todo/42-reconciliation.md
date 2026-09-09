# 42 — Reconcile allocations after inclusion

**Blocks on:** 41

## Files
- the ledger crate from ticket 41
- `crates/types/src/traits/bundles.rs` — reconstruct expected allocations
- `contracts/src/periphery/ControllerV1.sol` — `distributeFees`, unchanged

## Goal
Detect a bad allocation that already settled.

## Do
- Reconstruct expected allocations from each bundle's construction parent and the rates in force
  there; compare with the included reward updates and saved amounts.
- Report mismatches and missing reconstruction data, and withhold those amounts from any proposed
  distribution.
- Prove a proposed withdrawal leaves LP rewards and user balances backed — Angstrom's ERC20 balance
  is not the withdrawable amount.

## Done when
- A deliberately mis-split bundle is flagged and withheld.

## Notes
Detects after inclusion; cannot prevent or reverse settlement. A successful simulation and a
passing peer-finalization result are not evidence — peer checks run on `PoolSolution`s, upstream of
where splits are applied. Required before a nonzero ToB share (rollout step 5), not before step 4.
