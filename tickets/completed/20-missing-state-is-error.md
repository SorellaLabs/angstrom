# 20 — Unavailable state is an error, not a default

**Blocks on:** 19

## Files
- `crates/types/src/reth_db_wrapper.rs` — `storage_ref`, `basic_ref`, `code_by_hash_ref`, `block_hash_ref`

## Goal
Stop silent zeros standing in for missing state.

## Do
- `reth_db_wrapper.rs`: `storage_ref` returns `unwrap_or_default()`, so missing storage reads as
  zero. Surface it as an error instead.
- Same for the other accessors that default on absence, and the `.latest().unwrap()`.

## Done when
- A read against unavailable state errors rather than returning zero or falling back to current
  state.

## Notes
The silent zero had two sources, and the larger one was not the `unwrap_or_default()` calls.

`BlockHashReader`, `StateRootProvider`, `StorageRootProvider`, `StateProofProvider` and
`HashedPostStateProvider` all read `self.db.latest()`, so they answered from the current tip
whatever the selector said — the "falls back to current state" half of the requirement, and the one
that silently mixed two blocks in a single simulation. Every read now resolves through one private
`state()`, which is also what makes the ticket 19 selector mean anything. The
`StateProviderFactory` impl still delegates untouched: those methods hand out state for a block the
*caller* names and are not reads of the wrapper's own state.

`code_by_hash_ref` and `block_hash_ref` no longer default on absence — empty bytecode and a zero
block hash are both answers a caller cannot tell from a real one. `code_by_hash_ref` short-circuits
`KECCAK_EMPTY` first: an account with genuinely no code is not a failed read, and answering it must
not depend on state being available.

`storage_ref` keeps `unwrap_or_default()`, which is a departure from the ticket as written. Against
a state provider that resolved, `Ok(None)` means an unset slot, which the EVM reads as zero;
erroring there rejects every uninitialised storage read and no contract executes. The case the
ticket is about — a zero that is really state we failed to resolve — is upstream of the value and
now errors out of `state()` first, which `unavailable_state_errors_rather_than_reading_as_zero`
asserts for `storage_ref` alongside the others.

`basic_ref` still returns `Option`: revm needs to tell a non-existent account from an empty one, so
absence there is information rather than a failure.

`hashed_post_state` keeps its `unwrap()` — `HashedPostStateProvider` returns no `Result`, so there
is nothing to surface an error through. It now unwraps the selected block rather than the tip,
which is the part that mattered.
