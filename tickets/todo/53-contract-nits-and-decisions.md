# 53 — Contract nits, and two setter decisions to make on purpose

**Blocks on:** —
**Closes:** ISSUES.md 13 (PR #680 C.1–C.5)
**Follows:** 01, 10

## Overview
The reviewer's contract verdict is that it is safe to deploy, and every checkable part of that
holds. Five inline comments remain open on it: one warning to silence, two ABI additions that are
each a few lines but move PLAN.md's "three views" statement and ticket 10's ABI-shape test, and
two governance questions about the setter that the tree cannot answer. None is a defect. This
ticket does the free one, and makes the other four explicit decisions rather than things that
happened by default.

## Files
- `contracts/src/periphery/AngstromProtocolFeeConfig.sol:20,38-43,45,70-80,102`
- `contracts/test/AngstromProtocolFeeConfig.t.sol:416-500` — the ABI-shape tests
- `PLAN.md` — the contract block, selector list, and "three views"
- `crates/types/primitives/src/contract_bindings/mod.rs`, `abis-types/AngstromProtocolFeeConfig.sol/` — regenerate if the ABI moves

## Do
1. **C.3 — do it.** solc warning 2519: the constructor parameter `angstrom` (`:45`) shadows
   `function angstrom()` (`:102`). Rename it `angstrom_`. No ABI change.
2. **C.4 — decide.** `MAX_SHARE_E6` is `internal` (`:20`). `public` lets governance tooling read the
   denominator instead of hardcoding `1_000_000`, at the cost of a fourth view. If yes: update
   PLAN.md's "three views", ticket 10's `test_abiShape`, and regenerate bindings and `abis-types`.
3. **C.5 — decide.** `LpDonationSplitsSet` (`:38-43`) carries no sender. `address indexed sender`
   lets the change history tell timelock from multisig without a trace lookup. Changes the event
   signature; the eth manager decodes by field name and regenerates cleanly.
4. **C.2 — decide.** The setter writes the full pair unconditionally (`:79-80`), so a queued
   timelock execution silently reverts an intervening fast-owner change; PLAN.md pushes the guard
   to governance tooling. Compare-and-swap — `setLpDonationSplits(expectedUser, expectedTob,
   newUser, newTob)` reverting on mismatch — has precedent in
   `IAngstromAuth.removePool(StoreKey, PoolConfigStore expectedStore, uint256)`
   (`contracts/src/interfaces/IAngstromAuth.sol:29`). Changes the ABI and PLAN.md's selector list.
5. **C.1 — confirm.** The fast owner (a 2-of-N Safe on mainnet) can set both LP shares to 0% in one
   call, effective on the next bundle, with no timelock (`:70-77`). That is per PLAN.md ("Either may
   call"). Confirm the breadth is intended, or narrow it: fast owner may only *raise* LP shares, or a
   floor. Write the answer into PLAN.md's authorization paragraph either way.

## Done when
- `forge build` emits no warning 2519 for this contract.
- Each of C.1, C.2, C.4, C.5 has a recorded yes/no, and every "yes" has landed with PLAN.md, the
  ABI-shape test, the bindings and `abis-types` updated together.
- If any ABI change is taken, the deployed-contract question is answered too: the mainnet and
  Sepolia deployments (ticket 36) are of the *current* ABI, so an ABI change means a redeploy and a
  constants change, not just a source change.

## Notes
The reviewer attributed the CAS precedent to `ControllerV1.removePool`; it is on the `IAngstromAuth`
call that `ControllerV1.removePool` makes. The suggestion is unaffected.

Order matters: decide C.2/C.4/C.5 *before* touching the ABI, and decide them together, because each
one alone forces a redeploy and three redeploys is worse than one. If all three are "no", this
ticket is the one-line rename and a note.

C.1 is the only one with economic consequence and it is not a code question. PLAN.md's
authorization paragraph already says the fast-owner path bypasses the timelock by design; the
reviewer is asking whether "zero both shares instantly" was the intended breadth of that design or
an unexamined corner of it.
