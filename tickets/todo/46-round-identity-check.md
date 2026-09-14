# 46 — Reject results that do not match the round's parent and generation

**Blocks on:** —
**Closes:** ISSUES.md 6, the "discard" half, and acceptance criterion 1's head-change half
**Follows:** 21, 22, 23

## Files
- `crates/types/primitives/src/contract_payloads/angstrom/mod.rs:127-143` — `BundleGasDetails`, its
  `parent` field, the `parent()` accessor with no callers, and the `#[allow(unused)]`
- `crates/consensus/src/rounds/proposal.rs:102` — `try_build_proposal`, where the result arrives
- `crates/consensus/src/rounds/mod.rs:142` — `reset_round`
- `crates/consensus/src/rounds/mod.rs:204` — `SharedRoundState::block_height`
- `crates/consensus/src/rounds/mod.rs:306-378` — `matching_engine_output`, the capture point

## Goal
A result is built on only if it was produced against the parent, and in the generation, the round
is building for.

## Do
- In `try_build_proposal`, reject the matching result when
  `gas_info.parent() != handles.block_height` — no proposal, no bundle, no submission — and drop
  `#[allow(unused)]` from `BundleGasDetails` once `parent()` has a caller.
- Add a round generation to `SharedRoundState`, incremented in `reset_round`. Capture it in
  `matching_engine_output` beside the snapshot, carry it on `MatchingOutput`, and check it on the
  way back. PLAN.md: "a matching block height is not enough, since same-height reorgs exist" — the
  parent hash covers the reorg case, the generation covers a reset that lands on the same parent.
- Expose the identity to ticket 44's submission path, so the same check runs before signing and
  before each send.
- **Acceptance criterion 1's test.** Drive a round; change the head mid-round — once to a
  different height and once to a same-height different hash — and assert the stale result is
  rejected. Assert on the absence of a proposal, not on a flag.

## Done when
- A result carrying a parent other than the round's is discarded rather than built on.
- A result produced before a `reset_round` that landed on the same parent is also discarded.
- Same-height reorg mid-round: no proposal from the stale result.
- `#[allow(unused)]` is gone from `BundleGasDetails`.

## Notes
Ticket 21 put `parent: BlockNumHash` on `BundleGasDetails` so "a result carries the parent it was
produced against". Nothing consumes it: `from_proposal` takes the value as `_gas_details`
(`crates/types/src/traits/bundles.rs:848`). The identity is carried and dropped.

Stale *cross-round* results are already discarded structurally — `reset_round` replaces
`current_state`, dropping `ProposalState` and the futures it owns. What is missing is the
*intra-round* check, which is the one that catches ticket 43's race: a simulation whose parent was
moved under it returns a `BundleGasDetails` stamped with a parent that no longer matches, and today
nobody looks. Keep this check even after 43 lands — 43 removes the race, this proves it stays
removed.

The fee-config half of criterion 1 is already tested by
`a_config_update_mid_round_does_not_change_the_round_being_built`; this ticket adds the head-change
half beside it. `setup_state_machine` and `MockMatchingEngine` already give the test everything it
needs — the mock records the `parent_hash` it was handed.

Ticket 47 covers the other half of PLAN.md's parent-identity requirement: recording the
construction and inclusion parents.
