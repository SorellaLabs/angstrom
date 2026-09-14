# 39 — Close the pre-A replay equivalence question

**Blocks on:** —
**Closes:** ISSUES.md 3
**Follows:** 34, 25

## Files
- `crates/types/primitives/src/contract_payloads/protocol_fees.rs` —
  `the_deployed_split_reproduces_the_legacy_f64_path_below_its_bound`, and the `F64_DIVERGENCE`
  const it pins
- `testing-tools/src/replay/runner.rs:277` — the `load_from_chain` call replay already makes
- `bin/replay/src/lib.rs` — the replay entry point

## Goal
Answer whether any pre-**A** block actually diverges, with evidence.

## Do
1. **Answer the magnitude question first — it is the cheap half.** Ticket 34 reduced "would a
   recorded block diverge?" to "did any pre-**A** *per-pool* `total_user_fees` reach
   `3_002_399_751_580_333`?" Query the recorded data for the maximum per-pool `total_user_fees`
   across pre-**A** blocks. If the maximum is below the bound, the question is closed by argument
   and no replay run is needed — record the number and the query.
2. **Only if the maximum reaches the bound**, replay a spread of recorded pre-**A** blocks and diff
   each produced bundle against what was included. That is ticket 34's step 1 as originally written,
   and it needs S3 credentials, recorded block ids, and an archive fork URL.
3. **Only if step 2 shows a real divergence**, build ticket 34's step 2: branch in `process_solution`
   on `snapshot.block_number <= PROTOCOL_FEE_CONFIG_DEPLOYED_BLOCK`. That means threading the
   snapshot rather than just `DonationSplits`, which ticket 23 deliberately decided against — reopen
   23 explicitly rather than widening the signature quietly.

## Done when
- The maximum pre-**A** per-pool `total_user_fees` is a recorded number, not an assumption.
- Ticket 34's first "Done when" bullet — "A block either side of **A** replays identically to what
  was included" — is either satisfied by evidence or superseded by the magnitude argument with the
  number written down.

## Notes
Ticket 34 landed steps 1 (arithmetic half), 3 and 4, and explicitly did not land its empirical half:
"Replaying a spread of recorded blocks needs S3 credentials, recorded block ids, and an archive fork
URL, none of which were available; the first 'Done when' bullet is therefore unverified, and the
magnitude check is the cheap way to close it."

The arithmetic is already pinned and passing.
`the_deployed_split_reproduces_the_legacy_f64_path_below_its_bound` shows `(gross as f64 * 0.75)` and
`split_user(gross).0` agree for every value below `3_002_399_751_580_333`, with the bound tight —
the ten thousand values under it agree and the bound itself differs by exactly one unit. Below it
`3 * fees` fits in f64's 53-bit mantissa.

So this is not a re-derivation, it is a lookup. For an 18-decimal token the bound is 0.003 of it in
fees from a single batch, which is not obviously out of reach — that is why it is a real question
rather than a formality, and also why it is likely to come back "no" and close cheaply.

Step 3 is deliberately last and deliberately conditional. PLAN.md rollout step 6 asks for a legacy
`f64` path; ticket 34's argument is that the baked-in const already satisfies it, because
`load_from_chain` short-circuits to `(750_000, 1_000_000)` at or before the deployed block with no
provider call. The measurement is cheap and the dead branch is forever.
