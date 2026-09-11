# 39 — Replay either side of A

**Blocks on:** 12, 25

## Overview
Rollout step 6 requires replay before **A** to stay byte-exact, and PLAN.md frames that as
keeping the legacy `f64` path alive. This ticket's argument is that the requirement may already
be satisfied without one: `load_from_chain` short-circuits to the baked-in const at or before
the deployed block, so a pre-**A** replay runs the integer path at exactly the rates the `f64`
path used, and the two agree while `3 * total_user_fees` stays inside the f64 mantissa. So the
work is to measure first — replay a spread of recorded pre-**A** blocks and diff the produced
bundle against what was included — and add the branch only if a real divergence shows up. That
ordering matters because the branch is not free: it needs the snapshot threaded rather than just
`DonationSplits`, which ticket 23 deliberately decided against, so it means reopening that
decision rather than widening a signature quietly. The other half of the ticket is that
unreadable historical config must surface as a named gap per block, rather than aborting the run
or silently substituting today's rate. Allocation behavior needs nothing preserved — tickets 29
and 31 are reporting-only.

## Files
- `testing-tools/src/replay/runner.rs:277` — the existing `load_from_chain` call
- `bin/replay/src/lib.rs`
- `crates/types/src/traits/bundles.rs` — legacy vs. current path, only if step 2 says so

## Goal
Keep historical replay byte-exact.

## Do

1. **Measure before building anything.** Replay already loads rates from historical parent state:
   `runner.rs:277` calls `load_from_chain` with the replayed block's own number and hash, and
   blocks at or before **A** short-circuit to the baked-in const `(750_000, 1_000_000)` with no
   provider call (ticket 13). So a pre-**A** replay runs the integer path at exactly the rates the
   `f64` path used. Replay a spread of recorded pre-**A** blocks and diff the produced bundle
   against what was included.

2. **Add the legacy branch only if step 1 finds a divergence.** `x as f64 * 0.75` and
   `x * 750_000 / 1_000_000` agree exactly while `3 * total_user_fees` stays inside the f64
   mantissa (~`2^53`); above that they can differ by a unit. If no recorded block reaches that
   magnitude, there is nothing to preserve and no dead `f64` path to carry. If one does, branch in
   `process_solution` on `snapshot.block_number <= PROTOCOL_FEE_CONFIG_DEPLOYED_BLOCK` — which
   means threading the snapshot, not just `DonationSplits`, and ticket 23 deliberately decided
   against that. Reopen 23's decision rather than adding a second hash to the signature quietly.

3. **Missing historical state is a reported gap.** `load_from_chain` already errors on an empty
   account or a config bound to a different Angstrom. Ensure replay surfaces that as a named gap
   per block and keeps going, rather than aborting the run or substituting today's rate.

4. **Legacy allocation behavior needs nothing.** Tickets 29 and 31 are reporting-only — allocation
   policy is unchanged, so there is no pre-**A** allocation path to preserve.

## Done when
- A block either side of **A** replays identically to what was included.
- A block whose historical config cannot be read is reported as a gap, not silently replayed on
  today's rate.

## Notes
The one genuine behavior change that pre-**A** replay cannot reproduce is ticket 26 step 4: a ToB
order that fails `calc_vec_and_reward` used to yield a bundle with no ToB donation, and now errors
out of `process_solution`. If any recorded block hit that path, replay will now fail where the
node once produced a bundle. That is correct — the old behavior was a silently mispriced book swap
— but it is a replay divergence that no `f64` legacy branch would fix, so record it as a known gap
rather than chasing it.

Step 2 is deliberately conditional. PLAN.md rollout step 6 requires the legacy path; this ticket
asserts the requirement is already satisfied by the const and asks for evidence before adding code
to satisfy it twice. If the measurement shows otherwise, build it — but the measurement is cheap
and the dead branch is forever.
