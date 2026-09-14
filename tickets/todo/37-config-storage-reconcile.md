# 37 — Reconcile the log-derived config against storage at each head

**Blocks on:** —
**Closes:** ISSUES.md 2
**Follows:** 07, 13, 14, 15, 16

## Overview
PLAN.md's "Reading canonical state" item 3 says storage is the source of truth and the
`LpDonationSplitsSet` index is "never a second source of configuration". Tickets 07/14/15 inverted
that: storage is read once at init (16) and the pair is maintained from logs thereafter. The
inversion argument is sound — the setter writes the full pair and the event records both sides, so
a removed change is invertible from the log alone — and the log path is correct and well tested.
What it gives up is self-healing, and nothing in the node ever re-reads storage, so a pair that
drifts stays drifted silently. This ticket keeps the log path as the mechanism and adds storage
back as the check, which is the cheapest way to satisfy PLAN.md without unwinding three tickets.

## Files
- `crates/eth/src/manager.rs:220` — `apply_periphery_logs`, where the pair is currently decided
- `crates/eth/src/manager.rs:141` — `on_canon_update`, the once-per-notification seam
- `crates/eth/src/manager.rs:68` — `protocol_fee_config` on the cleanser
- `crates/types/primitives/src/contract_payloads/protocol_fees.rs` — `load_from_chain`, reused as is
- `bin/angstrom/src/components.rs:307-315` — the init read and the subscribe ordering

## Goal
A log-derived pair that disagrees with canonical storage is an error, not a silent divergence.

## Do
- Give the cleanser a provider handle so it can read at a pinned hash. `load_from_chain` already
  takes `(address, block_number, block_hash, provider)` and needs no change.
- Once per notification, after `apply_periphery_logs` has settled the pair, read slot 0 pinned to
  the notification tip's hash and compare against the pair about to be published.
- On disagreement: do not publish, do not overwrite with either value, and surface an error naming
  both pairs and the tip. A node that cannot agree with storage must not build affected bundles —
  same posture as ticket 17.
- Keep the log path as the mechanism. This is a check, not a replacement: the published value still
  comes from the logs so ticket 15's reorg inversion and ticket 22's ordering guarantee are intact.
- **Close the startup backlog at its source as well.** In `bin/angstrom/src/components.rs`, hand the
  cleanser the *original* `sub` (opened at `:266`) instead of the fresh `eth_data_sub` opened at
  `:313`, so every notification that queued during pool discovery is applied rather than dropped.
  Pin the init read to the block `sub.recv()` actually returned, then let the cleanser drain the
  rest of the queue before the first round. The reconcile above catches drift; this stops the
  largest known source of it.

## Done when
- A cleanser whose in-memory pair is forced out of step with storage errors on the next
  notification instead of publishing the stale pair.
- A notification with no setter still reconciles, so a pair lost to a gap is caught at the next
  head rather than never.
- A setter landing while `fetch_angstrom_pools` is running is applied before the first round.
  Drive startup with two or more blocks queued and assert the cleanser's first published pair
  reflects the later block's setter.
- Agreement costs one storage read per notification and changes no published value.

## Notes
The three drift paths this closes, none of which self-heal today:

- **Startup backlog** (PR #680 review, finding 4 — worse than a race). `components.rs:266` opens
  `sub`, `:286-291` runs `fetch_angstrom_pools` from the deploy block ("takes awhile", per the
  comment at `:303`), and every block that lands meanwhile queues on `sub`. `:307` `sub.recv()`
  returns the *oldest* queued notification — call it H — not the tip. `:313` then opens a fresh
  `eth_data_sub`, which sees only notifications published after that moment, and `sub` is dropped
  with everything still queued on it. A setter in any of H+1 .. tip is in neither the H storage
  read nor the cleanser's stream. The reviewer reproduced it against the real Tokio broadcast
  channel: selected parent 101, missed setter 102, first fresh notification 103. This is
  deterministic on every cold start whose pool discovery spans two or more blocks.
- **Missed receipts.** `logs_in_block_order` (`manager.rs:402`) uses
  `receipts_by_block_hash(..).unwrap_or_default()`, so a block whose receipts do not resolve is
  skipped without a word.
- **Reorg without the setter in `old`.** `reverted_protocol_fee_config` (`manager.rs:384`) returns
  `None` when the removed range does not carry the event, and the node keeps the reorged-out pair.

The subscribe-ordering and receipt-lookup plumbing predates this branch. What is new is that fee
configuration now depends on it with no recovery path.

**PLAN.md item 4 is the constraint to respect.** "The read must complete before consumers build the
corresponding round." The reconcile therefore has to sit inside the synchronous notification
handling, before `NewBlock` is sent — not on a detached task. That is also why it belongs in
`on_canon_update` rather than in a poller.

At the shipping rates `(750_000, 1_000_000)` nothing can drift, because the config never changes.
This is only live once rollout step 5 uses the setter, which is also when a wrong rate starts
costing money. Ship it before then, not after.

Alternative considered and rejected: replacing the log path with a storage read per head, as
PLAN.md literally describes. That deletes tickets 14 and 15's tested inversion logic and gains
nothing the reconcile does not already give.
