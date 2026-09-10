# 21 — Carry parent hash on bundle validation

**Blocks on:** 19

## Files
- `crates/validation/src/bundle/validator.rs` — `ValidationRequest::Bundle`
- `crates/types/src/reth_db_wrapper.rs`

## Goal
Simulate at H, execute at H+1, and say which H.

## Do
- `crates/validation/src/bundle/validator.rs`: add the parent hash to
  `ValidationRequest::Bundle`.
- Simulate at that state, execute in an H+1 environment, and return the identity with the result.
- Every read, including cache misses, resolves against the requested parent hash.

## Done when
- A result carries the parent it was produced against.
- A request for an unavailable parent errors.

## Notes
`simulate_bundle` takes the parent hash and resolves H from it with
`BlockNumReader::block_number`. That lookup *is* the availability check — an unknown hash gives
`None` and a provider failure gives `Err`, and both become errors before anything is pinned or
simulated. It also replaces the block number the validator used to read off
`order_validator.block_number`, so the H the bundle is priced at and the H+1 it executes in both
come from the parent the caller named rather than from whatever block the validator last saw.

`BundleGasDetails` carries a `BlockNumHash`, and its `Default` derive is gone: a defaulted result
would carry a zero parent, which is precisely the identity a caller must never be handed. The mock
matching engine echoes the parent it was given instead.

The caller side had no hash to pass. `MatchingEngineHandle::solve_pools` and
`MatcherCommand::BuildProposal` now carry one, sourced from `SharedRoundState::block_height`,
which — like `ConsensusManager::current_height` — became a `BlockNumHash` rather than gaining a
second field beside it: the two halves are only ever written together, so one value keeps them
from disagreeing by construction instead of by convention. That in turn needed
`EthEvent::NewBlock` and `EthEvent::ReorgedOrders` to carry `BlockNumHash` rather than a bare
number; the eth manager already had `tip_hash()`, nothing downstream of it did. The reorg arm
takes the new tip instead of `reorg.end()`, for the same no-disagreement reason. Ticket 24 carries
the `DonationSplitSnapshot` down the same path and ticket 25 adds the generation tag; both build
on this rather than replacing it.

`Validator`'s `db: Arc<DB>` field is removed — `set_block` was its only use.
`AnvilStateProvider::block_number` was `panic!("never used")` and is now implemented, since bundle
validation resolves its parent through it in the Anvil harness.

`simulate_bundle` points the db at that parent with `SetBlock` and then builds its `CacheDB`, so
every read of the simulation — cache misses included — resolves against the requested hash. Ticket
19 kept the selector shared and mutable, so "every read" holds at setup rather than for the life of
the simulation: a concurrent `set_block` can still move it. That gap is recorded in 19's notes.

Coverage: `a_result_carries_the_parent_it_was_produced_against` drives a real simulation and
asserts both halves of the identity; `simulation_reads_the_parent_it_was_handed` asserts the db was
pointed at the requested parent, by number and hash, before the cache was built.
`an_unavailable_parent_is_an_error` and `a_failed_parent_lookup_is_an_error` assert the failures
happen *before* that, so no bundle is ever simulated against a substitute state.
