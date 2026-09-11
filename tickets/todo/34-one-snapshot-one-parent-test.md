# 34 — One snapshot, one parent

**Blocks on:** 22

## Files
- `crates/consensus/src/rounds/mod.rs:53` — `MatchingOutput`
- `crates/consensus/src/rounds/mod.rs:142` — `reset_round`
- `crates/consensus/src/rounds/mod.rs:363-377` — the capture in `matching_engine_output`
- `crates/consensus/src/rounds/proposal.rs:102` — `try_build_proposal`
- `crates/consensus/src/rounds/mod.rs:632` — `setup_state_machine`

## Goal
Lock down the round invariant.

## Do

**This ticket has to build the mechanism it tests.** See Notes — the implementation ticket that
owned round identity was dropped in a renumber, and nothing else carries it.

1. **Round generation.** Add `round_generation: u64` to `SharedRoundState`, bumped in
   `reset_round` (`:142`) beside the existing `block_height` / `round_leader` writes. A generation
   is what distinguishes two rounds at the same height; the hash alone does not, since a round can
   be reset without the head moving.

2. **Tag the async result.** `matching_engine_output` already captures `parent_hash` and `splits`
   at `:367-368`, above the `async move`. Capture the generation there too and widen
   `MatchingOutput` (`:53`) to carry `(BlockNumHash, u64)` alongside what it returns now.

3. **Check on receipt.** In `try_build_proposal` (`proposal.rs:102`), compare the result's
   `(parent, generation)` against `handles.block_height` and `handles.round_generation`, and
   discard on mismatch rather than building. Re-check before signing and before each endpoint send
   in the submission future — ticket 35 shares this checkpoint.

4. **The same-snapshot half needs no new mechanism.** The snapshot rides out of
   `matching_engine_output` on `MatchingOutput` as one local `splits` used twice, so gas estimation
   and final construction cannot diverge by construction (tickets 22, 23). The test asserts it;
   nothing changes to make it true.

5. **Tests**, on `setup_state_machine`:
   - drive a round end to end, assert `MockMatchingEngine` recorded the same rates the round kept
     and that `from_proposal` got that same value, and that no second config or pool read happened;
   - reset mid-round to a **different hash at the same height** and assert the in-flight result is
     discarded.

## Done when
- The test fails if a second read is introduced.
- A same-height reorg mid-round rejects the stale result.

## Notes
**Provenance.** `tickets/todo/24-round-generation-identity.md` owned steps 1-3 and was deleted in
the `716f9e90` renumber rather than renumbered — everything after it shifted down by two, so the
loss is invisible in the sequence. PLAN.md still requires it under **Round semantics**, and
`grep -rn "generation\|cancel\|abort" crates/consensus/src/rounds/` returns nothing. Recover it
from git (`git show 7e8ea8fd:tickets/todo/24-round-generation-identity.md`) if a separate ticket is
preferred; otherwise it lives here.

`setup_state_machine` (`:632`) connects to `https://eth.llamarpc.com`. A test that depends on that
is not a test — point the provider at the Anvil harness or a stub before adding cases to it.

A matching block height is not enough on its own, which is why step 1 adds a generation *and*
step 3 compares the hash: same-height reorgs and reset-without-a-new-head are different failures
and neither field catches both.
