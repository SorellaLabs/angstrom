# 35 — Cancellation

**Blocks on:** 34

## Files
- `crates/consensus/src/rounds/proposal.rs:36` — the `submission_future` field
- `crates/consensus/src/rounds/proposal.rs:320` — `tokio::spawn(submission_future)`
- `crates/consensus/src/rounds/proposal.rs:358` — where it is polled
- `crates/consensus/src/rounds/mod.rs:142` — `reset_round`, which drops the state

## Goal
Prove a reset actually stops work.

## Do

**This ticket has to build the mechanism it tests** — see ticket 34's Notes on the dropped
implementation ticket.

1. **Keep the handle abortable.** `submission_future` is typed
   `Option<BoxFuture<'static, Result<bool, JoinError>>>` (`:36`) and `:320` boxes the `JoinHandle`
   into it. Boxing erases `abort()`, which is exactly why a reset cannot stop the task. Change the
   field to `Option<JoinHandle<bool>>` — a `JoinHandle` is already a `Future` with that same
   `Output`, so the poll at `:358` keeps working unchanged.

2. **Abort on drop.**

```rust
impl Drop for ProposalState {
    fn drop(&mut self) {
        if let Some(handle) = self.submission_future.take() {
            handle.abort();
        }
    }
}
```

   `reset_round` (`mod.rs:142`) replaces `current_state` wholesale, so dropping is the reset — no
   call site needs to change. Today that drop leaks a running task.

3. **Re-check identity inside the future**, after the async preparation and before each endpoint
   send, using ticket 34's `(parent, generation)`. Abort closes the window after the drop; the
   re-check closes the window before it, where the task is mid-`await` and has not yet been
   dropped.

4. **Tests.** Invalidate a round while matching, while simulating, while signing, and while an
   endpoint send is in flight. Assert against a recording submitter that **no further send
   happened** — count sends, do not assert a token exists. Include the dropped-handle case
   explicitly: drop a `ProposalState` with a live submission task and assert the task stops.

## Done when
- No later send or retry occurs in any of those phases.
- A dropped `ProposalState` aborts its submission task.

## Notes
Now blocks on 34 rather than nothing: step 3 needs the identity pair, and the ticket cannot assert
"no later send" without a way to say which round a send belongs to. The `Blocks on: —` it carried
was an artifact of the renumber that dropped its implementation ticket.

`abort()` is not instant — it cancels at the next await point, so a task already inside
`submit_tx` may still complete that call. That is the accepted limitation PLAN.md records: a
transaction already sent cannot be recalled. What this ticket closes is the *retry* and the
*second* send, not the one already in flight.
