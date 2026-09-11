# 34 — Replay either side of A

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
Ticket 26 left the `Err` arm of `calc_vec_and_reward` matching `main`, so a ToB order that fails
to evaluate still yields a bundle with no ToB donation rather than aborting the solution. That was
going to be the one behavior change pre-**A** replay could not reproduce; it is not one now, and
there is no known replay gap from the ToB path to record.

Step 2 is deliberately conditional. PLAN.md rollout step 6 requires the legacy path; this ticket
asserts the requirement is already satisfied by the const and asks for evidence before adding code
to satisfy it twice. If the measurement shows otherwise, build it — but the measurement is cheap
and the dead branch is forever.

**As built.** Steps 1, 3 and 4 landed. Step 2 did not, which is the outcome the ticket was
written to allow.

**Step 1's arithmetic half is measured and pinned; its empirical half is not run.**
`the_deployed_split_reproduces_the_legacy_f64_path_below_its_bound` in `protocol_fees.rs` drives
`DEPLOYED_INITIAL_PROTOCOL_FEE_CONFIG.splits` — the exact value a pre-**A** replay resolves to —
against `(gross as f64 * 0.75) as u128` and finds them identical for every `total_user_fees` below
`3_002_399_751_580_333`, with the bound tight: the ten thousand values under it agree and the bound
itself differs by exactly one unit. Below it `3 * fees` fits in f64's 53-bit mantissa, so the
product is exactly `3 * fees / 4` and truncating lands on the integer path's floor. Asserting the
LP half is asserting the whole split, since `save_amount` was the subtracted remainder on both
paths.

That reduces "would a recorded block diverge?" from a replay run to a magnitude check: did any
pre-**A** block's **per-pool** `total_user_fees` reach ~3.0e15 t0 units? For an 18-decimal token
that is 0.003 of it in fees from one batch, which is not obviously out of reach — so this is a real
question, not a formality. **It was not answered here.** Replaying a spread of recorded blocks needs
S3 credentials, recorded block ids, and an archive fork URL, none of which were available; the first
"Done when" bullet is therefore unverified, and the magnitude check is the cheap way to close it.

**Step 2 is not built, and ticket 23's decision stays closed.** No divergence has been demonstrated,
so there is no legacy `f64` branch, no snapshot threaded where `DonationSplits` suffices, and no
dead path to carry. If the magnitude check above turns up a block over the bound, that is when to
reopen 23 — not before.

**Step 3 names the gap; it does not make the run continue.** `runner.rs` called `load_from_chain`
and `?`-propagated, so an unreadable historical config surfaced as `no code at protocol fee config
address 0x…` with nothing tying it to a block. `load_replay_protocol_fee_config` now wraps it as
`replay gap at block {n}: historical protocol fee config at {addr} could not be read`, with the
cause chained. It has no fallback arm, which is the second "Done when" bullet: a gapped block is
never replayed on today's rate.

The "keeps going" half of step 3 is **deliberately not implemented**. One `ReplayCli` invocation
retrieves one snapshot and replays one block, so there is no next block within a run to keep going
to — "a named gap per block" is one attributable line per invocation, which is what an operator
replaying a spread actually reads. Continuing *within* a gapped block would mean making
`protocol_fee_config` optional on `EthDataCleanser` and `ConsensusManager`, which is ticket 17's
central decision ("no config means no bundle", non-optional by construction) and would weaken the
production node's fail-closed posture to serve replay. Not worth it, and not quietly.

Coverage: `an_unreadable_historical_config_is_a_named_gap` and
`a_gapped_block_is_not_replayed_on_todays_rate` in `replay::runner::tests`, driven through a mocked
provider that answers the code read with an empty account. Checked against the mutation that
removes the wrap — the naming test then sees the bare `no code at…` error, which is exactly the
failure mode the ticket describes.

**Step 4 needed nothing, as written.** Tickets 29 and 31 are reporting-only; 31's one behavior
change is a pair of `bail!`s on malformed range metadata that `reduce_ranges` cannot produce. There
is no pre-**A** allocation path to preserve.
