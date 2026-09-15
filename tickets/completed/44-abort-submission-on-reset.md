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

**As built.** `ProposalState` keeps the submission as a bare `JoinHandle<bool>` and implements
`Drop`: it cancels the round's `CancellationToken` and aborts the task. Replacing the state in
`reset_round` therefore aborts the submission instead of orphaning it; the `JoinHandle` is never
again merely dropped. `tokio_util::task::AbortOnDropHandle` was considered and not used — it needs
the `rt` feature in `consensus`, and the five-line `Drop` keeps both halves of a reset in one place.

**The token is the identity in the submission path.** The ticket asks for "a cancellation token
and the round's identity (parent plus generation)" to be threaded into `submit_tx`. Inside the
detached task there is no current identity to compare against, so the token — created with the
`ProposalState`, cancelled when it is dropped — is what "this round is still current" means there;
the parent-plus-generation comparison is ticket 46's, on the consensus side. `TxFeatureInfo`
carries the token (re-exported as `angstrom_types::submission::CancellationToken`) and it is
checked at the seams PLAN.md names: in `submit_tx` after the nonce, fee and chain-id reads; in
`build_and_sign_tx_with_gas` after the estimate returns and before signing (the angstrom
submitter's inline copy too); and before each endpoint send in the mempool, angstrom and mev-boost
submitters. Both mechanisms are kept because `abort` lands at the task's next yield, and between
the cancel and that yield the task may be about to create a send future whose first poll writes
the request to the socket.

Coverage. `a_reset_round_makes_no_further_sends` (`proposal.rs`) builds a proposal over a mocked
node (nonce, fee history, chain id answered by alloy's `Asserter`) and a counting submitter that
parks at a gate; the state is installed with `set_state_machine_at`, the real `reset_round` lands
while the submission is parked, the gate is released, and the count is asserted at zero. The mock
is deliberately blind to the token, so only the abort can keep it at zero — which is the
dropped-join-handle case: reverting `Drop` to a bare drop fails it. `a_round_that_is_not_reset_still_submits`
is the "exactly as before" control (one send, round ends on the outcome). `a_reset_round_is_not_signed`
(`crates/types/src/submission/mod.rs`) pins the before-signing check. The per-endpoint checks have
no test of their own: a cancel between signing and a send cannot be injected without a network.
The consensus tests now run over the mocked transport instead of `https://eth.llamarpc.com`.

Verification: `cargo nextest run -p consensus --lib rounds` — 13 passed; `cargo nextest run -p
angstrom-types --lib submission` — 1 passed; `cargo clippy -p consensus -p angstrom-eth -p
angstrom-types -p angstrom-types-primitives -p telemetry-recorder -p telemetry -p testing-tools
--all-targets -- -D warnings -A clippy::result_large_err -A mismatched_lifetime_syntaxes` — clean;
`cargo +nightly fmt` on the same crates. **Mutations:** `task.abort()` replaced by a bare drop of
the handle — `a_reset_round_makes_no_further_sends` fails (the parked submission counts one send);
the before-signing token check removed — `a_reset_round_is_not_signed` fails. Both restored;
`proposal.rs` and `submission/mod.rs` byte-identical to their pre-mutation copies (`cmp`).

**Review fixes** (independent review of the As-built, 2026-09-15). The anvil-gated acceptance
fixture (`crates/types/tests/anvil_settlement.rs`) did not compile against the new `TxFeatureInfo`
— its `#![cfg(feature = "anvil")]` kept it out of the `--all-targets` clippy run above. Fixed
(`Ok(30_000_000)`, `cancel: CancellationToken::new()`); `cargo check -p angstrom-types --features
anvil --tests` is clean. The angstrom submitter's attestation branch now checks the token before
signing as its bundle branch already did, and `AnvilSubmissionProvider` checks it before its send,
so every submitter — the harness one included — carries the pre-send guard. Accepted as documented
rather than changed: the counting test parks the submission at the endpoint seam only (a reset
during the nonce read or the estimate relies on the same abort, which tokio applies before the
task's next poll); the identity re-checked on the submission path is the token, not
parent-plus-generation. Reruns after the fixes: `cargo nextest run -p consensus --lib rounds` —
13 passed; `cargo nextest run -p angstrom-types --lib submission` — 1 passed.
Post-review: clippy with `--features anvil` (which compiles the fixture) — clean; the anvil settlement
test — 1 passed.
