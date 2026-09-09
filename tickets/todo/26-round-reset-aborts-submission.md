# 26 — Round reset aborts the submission task

**Blocks on:** 25

## Files
- `crates/consensus/src/rounds/proposal.rs:314` — `tokio::spawn(submission_future)`

## Goal
A reset must stop work in flight, not just stop listening to it.

## Do
- Abort the submission task on reset. Dropping the join handle leaves the task running.

## Done when
- Invalidating a round mid-flight produces no later send or retry.
