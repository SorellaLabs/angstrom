# 48 — Pin submission-time gas estimation to the construction parent

**Blocks on:** —
**Closes:** ISSUES.md 5 (PR #680 A.5)
**Follows:** — (the code is `main`'s; PLAN.md assigns the fix to this work)

## Files
- `crates/types/src/submission/mod.rs:186-230` — `submit_tx`: nonce by number, fees, chain id, the
  `bundle_gas_used` closure with bare `estimate_gas(tx).await.unwrap()`
- `crates/types/src/submission/mempool.rs:54-60` — `build_and_sign_tx_with_gas`, the consumer
- `crates/consensus/src/rounds/proposal.rs:125-135` — the call site, which has
  `handles.block_height` in scope and passes only `target_block`

## Goal
Submission preparation reads the same parent state as construction did, executes in an H+1
environment, and cannot panic.

## Do
- Add the construction parent (`BlockNumHash`) to `submit_tx`'s signature and pass
  `handles.block_height` from `try_build_proposal`. `target_block` is `H + 1` and stays.
- Pin the nonce lookup to the parent *hash*, not `target_block - 1` by number — a number cannot
  name one branch of a same-height reorg.
- Pin `estimate_gas` to the parent hash and give it the H+1 block environment (number, timestamp,
  base fee), so it simulates the same thing bundle validation simulated. Alloy's default for a bare
  `estimate_gas` is `pending`, which is neither.
- Replace the `.unwrap()` with `?`. An estimation failure is a failed submission, not a panic in a
  spawned task.

## Done when
- The nonce and the gas estimate are both resolved against the construction parent's hash.
- The gas estimate runs in an H+1 environment.
- A provider error during estimation surfaces as `Err` from `submit_tx` and is recorded by the
  existing submission metrics, and the task does not panic.
- A submission whose parent is no longer canonical at estimation time is observable as such.

## Notes
PLAN.md, Round semantics: "Submission-time `estimate_gas` needs the same parent state and H+1
environment." `crates/types/src/submission/mod.rs` is untouched by this branch; the reviewer notes
the same ("This code predates the PR but remains an explicit, unimplemented handoff requirement").

Today `:196-198` pins the nonce by number and `:212` does not pin the estimate at all. A reorg or
head advance between construction and submission therefore changes the state used to prepare the
transaction, and the gas limit actually sent is derived from whatever the endpoint considers current.

This is the ticket that gets the parent into the submission path; tickets 44 (identity re-check
before signing and each send) and 47 (record the construction parent) both consume what this
threads. Land this first.

Bundle validation's `simulate_bundle` already builds the H+1 environment from the parent
(`crates/validation/src/bundle/mod.rs`); reuse its shape rather than inventing a second one.
