# 44 — Abort the submission task on round reset

**Blocks on:** —
**Closes:** ISSUES.md 1 (PR #680 A.2), and acceptance criterion 2
**Follows:** — (the code is `main`'s; PLAN.md assigns the fix to this work)

## Overview
`ProposalState` stores its submission work as a boxed `tokio::spawn` `JoinHandle`. `reset_round`
replaces the state and drops the handle — and dropping a `JoinHandle` detaches the task, it does
not cancel it. The detached task goes on to look up the nonce, estimate gas, sign, and send to
every endpoint, for a round that has already been invalidated. PLAN.md names this exact hazard
("a dropped handle leaves the task running") and asks for an abort plus identity re-checks
"after async preparation, before signing, and before each endpoint send". None of that exists,
and nothing in `crates/consensus/src` mentions cancellation. This is the one live correctness
bug in the set, and it is `main`'s: `proposal.rs:314` there is the identical `tokio::spawn`.

## Files
- `crates/consensus/src/rounds/proposal.rs:36` — `submission_future: Option<BoxFuture<.., Result<bool, JoinError>>>`
- `crates/consensus/src/rounds/proposal.rs:208-320` — the submission future and its `tokio::spawn`
- `crates/consensus/src/rounds/proposal.rs:341-375` — `poll_transition`, where the handle is polled
- `crates/consensus/src/rounds/mod.rs:142` — `reset_round`
- `crates/types/src/submission/mod.rs:186-230` — `submit_tx`: nonce, fees, chain id, then submitters
- `crates/types/src/submission/mempool.rs:41-90` — `submit`: sign, then fan out to every client

## Goal
A reset round makes no further sends, and a test proves it by counting sends that did not happen.

## Do
1. **Own the task.** Keep the `JoinHandle` (or a `tokio_util::sync::CancellationToken` plus the
   handle) on `ProposalState` and implement `Drop` to `abort()` it, so that replacing the state in
   `reset_round` cancels the task instead of orphaning it. Dropping a `JoinHandle` is never again
   the mechanism.
2. **Re-check identity at the seams PLAN.md names.** Thread a cancellation token and the round's
   identity (parent `BlockNumHash` plus the generation ticket 46 adds) into `submit_tx`. Check it
   after `build_and_sign_tx_with_gas` returns and before signing, and again before each
   `send_raw_transaction` in `mempool.rs` (and the equivalent in every other submitter). A stale
   identity or a cancelled token returns without sending.
3. **Cover the dropped-join-handle case explicitly.** The test must fail if step 1 is reverted to
   a bare `drop`.

## Done when
- Invalidating a round while nonce lookup, estimation, signing, or an endpoint send is in flight
  produces no later send and no retry.
- The assertion is on sends that did not happen — a submitter mock that counts calls — not on the
  presence of a token.
- Reverting the abort (letting the handle drop) fails the test.
- A round that is *not* reset still submits exactly as before.

## Notes
PLAN.md's **Accepted limitation** covers transactions already handed to an endpoint — "a
transaction already sent cannot be recalled". This ticket is about the sends that had *not yet*
happened at reset time, which the reviewer was careful to separate: "These are sends not yet made
when reset occurred, outside the accepted already-submitted limitation." Do not let the limitation
be used to wave this off.

`submit_tx` already awaits three provider calls before the first send (nonce, fees, chain id), and
`mempool.rs` awaits signing before fanning out — that is the window. On a 12-second slot it is not
small.

The `BoxFuture<'static, Result<bool, JoinError>>` type on the field is the tell: a `JoinError` can
only come from a spawned task. If step 1 replaces the spawn with an inline future polled by
`poll_transition`, that type changes and the "dropped join handle" case ceases to exist by
construction — that is also an acceptable design, provided the identity re-checks in step 2 still
land, since the future would then be dropped at reset rather than aborted. Either way, write the
test first.

Ticket 46's generation counter is what makes "identity" mean something when a reset lands on the
same parent; if 46 has not landed, use the parent hash alone here and note it.
