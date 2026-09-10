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
