# 34 — Accrual ledger from canonical bundles

**Blocks on:** 27

## Overview
A new module in `crates/types` answering what the protocol is owed, derived from chain state
rather than from telemetry about what the builder meant to do. It accrues from bundles decoded
out of included blocks' calldata on the same canonical commit/reorg feed the eth cleanser uses,
processing every block in a notification rather than just the tip — `apply_periphery_logs` had
exactly that bug. Accruals key on `(block_hash, tx_hash, pool_id, token)`, because block number
is not a key when same-height reorgs can give two different bundles the same one. That key is
also what makes a reorg a delete rather than an inversion, and what makes restart and backfill
idempotent by upsert. Accrual happens on commit so the ledger stays current, but rows only
become withdrawable at finalization, so a reorg can never remove a row that was already
withdrawable. Nothing here holds withdrawal authority; the module produces numbers a human reads
before an operator-reviewed timelock execution.
## Files
- `crates/types/src/fee_ledger.rs` (new module) — the ledger; owning operator settled in this ticket
- `crates/types/src/traits/chain_ext.rs` — `ChainExt`, which needs a per-block transaction walk
- `crates/eth/src/manager.rs:320` — `fetch_filled_order`, the existing bundle decode to reuse
- `crates/eth/src/manager.rs:402` — `logs_in_block_order`, the every-block pattern to follow
- `crates/eth/src/manager.rs:150,209` — `handle_reorg` / `apply_periphery_logs`, the commit/reorg feed

## Goal
Know what is owed, from chain state rather than telemetry about intent.

## Do

1. **Source is canonical inclusion.** Subscribe to the same canonical commit/reorg feed the eth
   cleanser uses, and accrue from bundles found in included blocks — decoded from the transaction
   calldata, not from proposal or submission telemetry. Process every block in a notification, not
   just the tip; `apply_periphery_logs` had exactly that bug (ticket 14).

2. **Key every accrual by `(block_hash, tx_hash, pool_id, token)`.** Block *number* is not a key —
   same-height reorgs give two different bundles the same number. This key is also what makes
   step 4 hold.

3. **Reorgs undo and re-derive.** On a reorg, delete every accrual whose `block_hash` is in the
   old chain, then apply the new chain's blocks. Storage keyed by hash makes this a delete rather
   than an inversion — do not try to invert amounts.

4. **Idempotent by construction.** Writes are upserts on the step-2 key, so a restart that
   re-processes a block, a backfill that overlaps, and a re-reviewed proposal all converge on the
   same row instead of adding a second one. Persist a watermark (last finalized block processed)
   so a restart knows where to resume, but correctness must not depend on the watermark being
   right.

5. **Finalization gates withdrawal, not accrual.** Accrue on commit so the ledger is current;
   mark rows withdrawable only at `EthEvent::FinalizedBlock`. A reorg can only ever remove a row
   that was not yet withdrawable.

## Done when
- Restart and backfill produce the same totals.
- A reorg that removes an included bundle removes its accrual.
- The same fee cannot be collected twice across restart, backfill, or a re-reviewed proposal.

## Notes
The owning operator is a deliverable of this ticket, not a precondition — PLAN.md requires it
named in the rollout artifacts before a nonzero ToB share.

**It lives in `crates/types`, not a new crate and not `crates/eth`.** Not a new crate, because
everything step 1 needs already exists: `fetch_filled_order` (`manager.rs:320`) filters
transactions to the Angstrom address and decodes `executeCall` calldata through
`AngstromBundle::pade_decode`, and `handle_reorg` already diffs old against new. A separate
crate would depend on all of it and duplicate the decode.

Not `crates/eth` either, because the ledger is a pure function of a chain notification and a
bundle — `AngstromBundle`, `AngstromPoolConfigStore` and `ChainExt` are the whole of its input,
and all three are `angstrom-types`. `crates/eth` owns the *feed*, not the derivation, and it
already depends on `angstrom-types`, so the cleanser drives `on_commit` / `on_reorg` from where
it handles notifications without the accrual arithmetic living beside the subscription. This also
puts the ledger beside `bundles.rs`, whose `process_solution` is what ticket 35 reconciles
against. Not `angstrom-types-primitives`: the ledger walks a `ChainExt`, which needs
`reth-provider`, `reth-ethereum-primitives` and `reth-primitives-traits` — three deps the leaf
crate deliberately does not carry, and pulling reth down into it to host a consumer of the trait
would be the wrong direction.

One thing to fix rather than copy: `fetch_filled_order` reads
`successful_tip_transactions()` — the tip block only. The ledger must walk every block in a
notification, the same bug ticket 14 fixed for logs. Follow `logs_in_block_order`
(`manager.rs:402`), which iterates `block_hashes()`, rather than the tip-only helper.

There is no per-bundle telemetry to accrue from, and there would be no point if there were:
accruing from what the builder recorded would mean the ledger agrees with it by construction,
and ticket 35's whole purpose is to detect a builder that got it wrong. Calldata is the source.

Nothing here holds withdrawal authority or initiates a distribution. `distributeFees` stays
owner-only and operator-reviewed; this module produces numbers a human reads.

**As built.** All five steps landed. `crates/types/src/fee_ledger.rs` holds `AccrualKey`,
`Accrual` and `FeeLedger`; `EthDataCleanser` holds one and drives `on_commit` / `on_reorg` from
`handle_commit` / `handle_reorg`, so the ledger rides the same feed as the cleanser without
subscribing separately.

**`ChainExt` had no per-block transaction accessor, so step 1 needed a new one.** Ticket 14 could
fix the same bug for logs with a free function because `receipts_by_block_hash` and
`block_hashes` were already on the trait; transactions were reachable only through
`successful_tip_transactions` and `tip_transactions`, both tip-only. `ChainExt` gains
`successful_transactions_in_block_order`, returning each transaction paired with the
`BlockNumHash` it landed in — the identity has to come back with the transaction because step 2
keys on it and a consumer cannot recover it afterwards. The item lifetime is named explicitly
rather than elided: elided inside the tuple it resolves against `'static`, and `alloy`'s
`Transaction` trait is `'static`-bound, so `tx.to()` on a borrowed item stops compiling.

**`save` is per token, not per pool — the one deviation from step 2's key.** A bundle encodes
`Asset.save` once per token and it cannot be attributed across two pools that share a token0, so
the row carries it on the first pool in the bundle's own pair order that names the token and zero
on the rest. Pair order is fixed at encode time, so this is deterministic across a re-walk, and
`two_pools_sharing_token0_record_save_once` asserts a sum over rows recovers the bundle's `save`
exactly once — which is what "the same fee cannot be collected twice" requires. Everything that
*is* per pool — the `RewardsUpdate` total and the ToB order's gas — stays on its own row.

**The watermark is in memory, not persisted.** Step 4 asks for it persisted, and there is no
storage layer in this repo to persist it to; adding one is more than this ticket. What step 4
actually requires still holds, because it is the half that does not depend on persistence: every
write is an upsert on the step-2 key, so a restart that re-walks blocks and a backfill that
overlaps both converge on the same row.
`reprocessing_a_block_produces_the_same_totals` asserts exactly that by running the same
commit twice. A restart today rebuilds by re-walking rather than resuming, which is slower and
correct; a durable store is the follow-up, and it is bounded by "correctness must not depend on
the watermark being right" already being true.

**Nothing emits `EthEvent::FinalizedBlock`.** `FeeLedger::finalize` is the gate step 5 asks for
and it works, but the only producer in the tree is `testing-tools`' mock — `reth`'s
`CanonStateNotification` carries no finality, and `pool_manager.rs:300` has consumed a variant
nothing sends since before this branch. So in a running node nothing is ever marked withdrawable.
That fails closed, which is the right direction for a ledger a human reads before a distribution,
but it is a gap to close before rollout step 5 rather than a property to rely on.

**The owning operator is not named.** It is a deliverable of this ticket per PLAN.md, and it is
the one thing here that cannot be settled in code. Recorded as an open rollout-artifact item: a
nonzero ToB share needs a named human before it is enabled, and this ticket does not supply one.

Read surface: `EthCommand::FeeLedgerAccruals` and `Eth::fee_ledger_accruals` return every row.
Read-only by construction — the command carries a `oneshot::Sender` and no mutation path — which
is what "produces numbers a human reads" means mechanically. `distributeFees` stays owner-only
and operator-reviewed; nothing added here holds withdrawal authority.

`MockChain` moved from `crates/eth/src/manager.rs`'s test module to `traits::mock` beside the
trait it implements, and gained `MockAncestor` so an ancestor block can carry transactions rather
than only receipts. It is plain `pub` rather than `#[cfg(test)]`, matching how `crate::testnet`
already ships fixtures: the alternative is each crate growing its own mock, and a second mock is a
second chance to implement `successful_transactions_in_block_order` tip-only and have the tests
agree with the bug.

Coverage, each checked against the mutation that should break it:
`accrues_from_every_block_not_only_the_tip` fails when `on_commit` reverts to
`successful_tip_transactions`; `two_pools_sharing_token0_record_save_once` fails when the
per-token dedupe is dropped; `reprocessing_a_block_produces_the_same_totals`,
`a_reorg_removes_the_accrual_it_dropped` (at the same height, different hash — the case a block
number cannot express) and `finalization_gates_withdrawal_not_accrual` cover steps 3-5. Each
mutation failed only the test that owns it.
