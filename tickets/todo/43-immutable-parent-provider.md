# 43 — Immutable state provider per parent hash

**Blocks on:** —
**Closes:** ISSUES.md 3 (PR #680 A.1)
**Follows:** 19, 20, 21

## Overview
Ticket 19's two unmet "Done when" clauses, reopened — this time with the reviewer's evidence that
the sharing bites in both directions. Ticket 19 made the selector a `BlockNumHash` and ticket 20
routed every read through one `state()` that errors instead of answering from the tip; both
landed. What did not is immutability: `RethDbWrapper.block` is one `Arc<RwLock<BlockNumHash>>`
shared by every clone, and `crates/validation/src/lib.rs:104-124` hands that one wrapper to order
validation, `FetchUtils` and the bundle validator alike. `simulate_bundle` then `set_block`s it
for a historical parent and spawns onto a thread pool, while `ValidationRequest::NewBlock`
`set_block`s it to the new head. So a `NewBlock` moves an in-flight bundle simulation forward, and
a queued historical simulation moves *live order validation backward*. PLAN.md asks for an
immutable provider per parent hash; the branch got to "hash-addressed and error-surfacing" and
stopped one step short.

## Files
- `crates/types/src/reth_db_wrapper.rs:25-61` — `SetBlock`, the selector, `RethDbWrapper::new`, `state()`
- `crates/types/src/reth_db_wrapper.rs:594` — `set_block_moves_every_clone`, the test to invert
- `crates/validation/src/lib.rs:104-124` — the one `revm_lru` handed to all three consumers
- `crates/validation/src/bundle/mod.rs:98-130` — `simulate_bundle`, the pin-then-spawn site
- `crates/validation/src/validator.rs:137-149` — `ValidationRequest::NewBlock`, the other writer
- `crates/validation/src/validator.rs:127` — `ValidationRequest::Bundle`, which already carries the hash

## Goal
A simulation reads exactly one parent for its whole life, and nothing it does is visible to order
validation.

## Do
1. Make the selector immutable. `RethDbWrapper::new(db, block)` already takes the pair; hold it by
   value and delete `SetBlock` / `set_block`. A wrapper for a different parent is a new wrapper.
2. `simulate_bundle` (`bundle/mod.rs:125`) currently `set_block(parent)`s the shared wrapper and
   clones. Construct a fresh wrapper for `parent` from the underlying factory instead, build the
   `CacheDB` on that, and hand it to the thread pool — nothing the simulation touches is reachable
   from another request.
3. Order validation keeps a head-following view, but as its own wrapper rebuilt on each
   `NewBlock`, not a mutation of a shared one. `validator.rs:146` resolves the hash with
   `self.db.block_hash(block_number).unwrap().unwrap()` — replace both unwraps with an error.
4. Invert `set_block_moves_every_clone` into the assertion that it cannot.

## Done when
- Two wrappers for different parents cannot influence each other.
- No API remains that mutates a selector after construction.
- A `ValidationRequest::NewBlock` arriving while a bundle simulation is queued or running cannot
  change which parent that simulation reads — assert on the parent the simulation *resolved*, not
  on the absence of a call.
- A queued historical bundle simulation cannot move the parent order validation reads.
- A failed block-hash lookup on the order path is an error, not a panic.

## Notes
The reviewer reproduced the clone interference in both directions against the actual
`RethDbWrapper` and `MockEthProvider`, and was careful to call it "selector proof, not a full EVM
race reproduction". That is the right level: the selector *is* the bug, and this ticket is closed at
the selector.

**This branch already improved this a lot** — on `main`, `simulate_bundle` did not pin at all and
took its block number from `order_validator.block_number`; the selector was an `Arc<AtomicU64>`.
The remaining gap is a race rather than an absence, which is why it is worth closing rather than
redesigning.

Scope stays ticket 19's: the selector and its caches, not a wider provider-layer rework. The
`StateProviderFactory` impl still delegates untouched — those methods hand out state for a block the
*caller* names.

Caches were already fine and stay as they are: `BundleValidator` holds a `CacheDB` but
`simulate_bundle` takes `&self` and mutates only its local clone, so every simulation starts cold.
Building the `CacheDB` on the per-parent wrapper (step 2) makes that structural rather than
incidental. `hashed_post_state` keeps its `unwrap()` — `HashedPostStateProvider` returns no `Result`.

Ticket 46 adds the identity check that proves, per result, that a simulation's reads used the parent
it was handed. Do both; this removes the race, 46 keeps it removed.
