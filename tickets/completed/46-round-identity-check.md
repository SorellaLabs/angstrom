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

**As built.** `SharedRoundState::generation` is bumped in `reset_round`, captured in
`matching_engine_output` beside the splits, and checked in `try_build_proposal` together with the
parent: a result is built on only if `output.gas.parent().hash == handles.block_height.hash` and
`output.generation == handles.generation`. Otherwise it is discarded at warn level before a
proposal, a bundle, an attestation or a submission exists. `#[allow(unused)]` is gone from
`BundleGasDetails`; it was also covering `total_gas_cost_wei`, which nothing reads, so that field
gained a `gas_used()` accessor rather than a narrower allow.

`MatchingOutput` is a struct, not the tuple: with the generation (and ticket 45's pool map) it
would have been five wide and destructured in three places. `finalization.rs` reads `.solutions`.

**Hash, not the full `BlockNumHash`.** The ticket says `gas_info.parent() != handles.block_height`.
The comparison is on the hash alone: a hash names exactly one block, so it is the same check, and
`MockMatchingEngine` cannot resolve a height and echoes `0` for it, which would have rejected every
result in the tests.

**Exposed to ticket 44.** Inside the detached submission task there is nothing to compare an
identity against, so the identity the submission path re-checks is the `CancellationToken` created
with the `ProposalState` and cancelled when it dies — see 44. The parent-plus-generation comparison
lives here, on the consensus side, where `handles` is.

Coverage, `crates/consensus/src/rounds/proposal.rs`: acceptance criterion 1's head-change half is
`a_result_for_a_parent_the_head_moved_from_is_not_built_on`, which drives a capture, moves the head
through the real `reset_round` — once to height 2, once to a different hash at height 1 — and
asserts no proposal, no submission task and no propagated message. The two checks are also pinned
one at a time: `a_result_stamped_with_another_parent_is_not_built_on` (a foreign parent in the
current generation) and `a_result_from_before_a_reset_on_the_same_parent_is_not_built_on` (the
same parent, one reset later). `a_round_that_is_not_reset_still_submits` is the control that a
current result *is* built on.

Verification: `cargo nextest run -p consensus --lib rounds` — 13 passed; clippy and fmt as recorded
on ticket 44. **Mutations:** the parent-hash comparison removed —
`a_result_stamped_with_another_parent_is_not_built_on` fails; the generation comparison removed —
`a_result_from_before_a_reset_on_the_same_parent_is_not_built_on` fails. Each restored;
`proposal.rs` byte-identical to its pre-mutation copy.

**Review fixes** (independent review of the As-built, 2026-09-15). The doc link to the private
`generation` field is plain code font now (`cargo doc -p consensus` warned). The coverage claim is
corrected: reached through `poll_transition` the check cannot trip, because `reset_round` replaces
the state and the matching future is dropped with it — so it is a tripwire reachable only by direct
call, which is how the three tests reach it, and the parent it compares is validation's echo of the
hash it was asked for, not an observation of the state it read. What excludes stale results in
production is that structural drop, pinned by ticket 44's `a_reset_round_makes_no_further_sends`;
this check is what would catch it if the structure ever changed, which is the ticket's stated
intent. Left as is: `gas_used()` has no caller (kept over a field-level allow); the build-time
metric and `last_round_info` are recorded before the check, as they already were for the `Err`
path. Rerun: `cargo nextest run -p consensus --lib rounds` — 13 passed.
