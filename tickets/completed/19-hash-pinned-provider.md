# 19 — Pin the state provider by parent hash

**Blocks on:** —

## Files
- `crates/types/src/reth_db_wrapper.rs` — `RethDbWrapper`, `SetBlock`, the `Arc<AtomicU64>` selector and every `state_by_block_id` call

## Goal
Simulate at one exact parent state, including on same-height reorgs.

## Do
- `crates/types/src/reth_db_wrapper.rs`: `RethDbWrapper` selects state with
  `state_by_block_id(self.block.load(...).into())` — a block *number* — held in an
  `Arc<AtomicU64>` shared by every clone, and `set_block(&self)` moves all of them at once.
- Replace the selector with an immutable parent hash fixed at construction. Change the selector
  type; a `u64` cannot express a same-height reorg.
- Caches must not carry state across hashes.

## Done when
- Two wrappers for different parents cannot influence each other.
- No API remains that mutates the selector after construction.

## Notes
Scope is the selector and its caches. Not a wider provider-layer rework.

**The selector type changed; its mutability did not.** `Arc<AtomicU64>` is now
`Arc<parking_lot::RwLock<BlockNumHash>>`, and `SetBlock::set_block` takes a `BlockNumHash`. Every
read resolves through `state_by_block_id(self.block.read().hash.into())`, so state is selected by
hash and a same-height reorg is expressible — the thing a `u64` could not do. `RethDbWrapper::new`
takes the pair, and `block()` reads it back.

The two "Done when" clauses are **not** met, deliberately. The selector is still shared by every
clone and still moves under them, so two wrappers on different parents remain the same wrapper, and
`set_block` remains an API that moves it after construction. Making them hold means the wrapper
stops being the head-following view that order validation needs — `ValidationRequest::NewBlock`
carries only a block number and nothing upstream of it carries a hash, so pinning it immutably
means threading a hash through the order pool, which is the wider rework this ticket rules out.
The race that leaves open is a `set_block` landing between a simulation's setup and its reads.

What *is* closed is the part that cost nothing: the selector names a branch rather than a height.
Ticket 20 then routes every read through one resolver, without which the selector would not reach
half of them.

"Caches must not carry state across hashes" was already true and is left as it was.
`BundleValidator` holds a `CacheDB`, but `simulate_bundle` takes `&self` and mutates only its local
clone, so the field stays empty and every simulation starts cold. Nothing needed to change; the one
thing to watch is that a future `&mut self` there would silently start persisting cache across
blocks.

`set_block_moves_every_clone` pins the current behaviour rather than the wanted one: it asserts a
clone moves the original, at the same height and a different hash. It is the test to invert if the
unmet clauses are taken up.

The unmet clauses are worth reopening as their own ticket if the round-identity work in 25 makes a
hash available on the order path.
