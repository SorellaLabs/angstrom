# 45 — Carry the round's pool snapshots through final construction

**Blocks on:** —
**Closes:** ISSUES.md 4 (PR #680 A.3)
**Follows:** 22, 23

## Files
- `crates/consensus/src/rounds/mod.rs:53` — `MatchingOutput`
- `crates/consensus/src/rounds/mod.rs:288-304` — `fetch_pool_snapshot`
- `crates/consensus/src/rounds/mod.rs:363` — the capture for matching and gas estimation
- `crates/consensus/src/rounds/proposal.rs:160` — the second fetch, for final construction
- `crates/consensus/src/rounds/proposal.rs:102-200` — `try_build_proposal`

## Goal
Gas estimation and final construction use the same pool state, by construction.

## Do
- Widen `MatchingOutput` to carry the `pool_snapshots` map captured at `rounds/mod.rs:363`,
  exactly as it already carries the `DonationSplitSnapshot`.
- In `try_build_proposal`, delete the `handles.fetch_pool_snapshot()` at `proposal.rs:160` and use
  the carried map for `from_proposal`.
- Assert it: extend `a_config_update_mid_round_does_not_change_the_round_being_built` (or add a
  sibling) so that a pool mutated between matching and final construction does not change the
  bundle — `MockMatchingEngine` already records what it was handed, so compare the map `from_proposal`
  used against the map matching ran on.

## Done when
- `fetch_pool_snapshot` is called exactly once per round.
- A pool update landing between matching and final construction — from a block *or* from
  `load_more_ticks` — does not change the bundle built.
- PLAN.md acceptance criterion 1's "neither re-read config or pool state" holds for pool state.

## Notes
This is the pool half of the requirement whose config half ticket 22 closed. Ticket 22's own
reasoning applies verbatim: carrying the value on the result "is what makes 'the same value'
structural: there is one capture in one tuple, so the value matching was driven on and the value
final construction reads cannot diverge, rather than being two reads that happen to agree."

Both `fetch_pool_snapshot()` calls are on `main` (`proposal.rs:161` there); the branch did not
introduce the double read, it inherited it and did not close it.

**Why it is reachable without a block boundary.** `SyncedUniswapPools` is written under a lock from
the pool manager's own task — `pool_update_workaround` on each block, and `load_more_ticks` whenever
`calculate_rewards` extends a pool's loaded tick range
(`crates/uniswap-v4/src/uniswap/pool_manager.rs:303,317`). `calculate_rewards` is called by order
validation for every incoming ToB order (`crates/validation/src/order/state/mod.rs:155`), so it
fires on ordinary order arrival throughout the round. And `ConsensusManager::poll`
(`crates/consensus/src/manager.rs:330-352`) drains the block stream before polling the round state,
but the pool manager does not wait for it, so `try_build_proposal` can read post-block pools while
the pre-block round is still current.

Ticket 46's identity check is the other half of the reviewer's requested correction ("reject
obsolete round results by identity"); this ticket is the "carry the original pool snapshots" half.

**As built.** `MatchingOutput` carries `pool_snapshots`, captured in `matching_engine_output` on the
same line as the splits; `solve_pools` is handed a clone of that local and the struct carries the
original, so the value matching ran on and the value final construction reads are one capture.
`try_build_proposal`'s second `fetch_pool_snapshot()` is deleted and `from_proposal` takes
`&output.pool_snapshots`. `fetch_pool_snapshot` has exactly one production call site
(`grep -n fetch_pool_snapshot crates/consensus/src/rounds/mod.rs`: the definition,
`matching_engine_output`, and the test below), and `matching_engine_output` runs once per round,
from `ProposalState::new`.

`MockMatchingEngine` was not extended to record the pools it was handed, as the ticket suggested:
the map `solve_pools` receives is `pool_snapshots.clone()` of the same local the struct carries, so
"the same map" holds by construction rather than by a comparison.

Coverage: `a_pool_update_mid_round_does_not_change_the_pools_the_round_builds_on` puts a real
`EnhancedUniswapPool` (over `DataLoader::default()`, liquidity 1 000) into `SyncedUniswapPools` with
a registry entry, takes the round's capture, writes liquidity 2 000 under the pool's lock while the
round is in flight — the `load_more_ticks` shape — and asserts the carried snapshot still says
1 000 while a fresh `fetch_pool_snapshot` says 2 000. `setup_state_machine_with` grew a
`UniswapPoolRegistry` parameter and configures every registry pool into the angstrom pool store so
`fetch_pool_snapshot` can resolve it.

Verification: `cargo nextest run -p consensus --lib rounds` — 13 passed; clippy and fmt as recorded
on ticket 44. **Mutation:** the carried map re-read from the live pools at completion instead of at
capture — `a_pool_update_mid_round_does_not_change_the_pools_the_round_builds_on` fails (`2000`
against `1000`). Restored; `rounds/mod.rs` byte-identical to its pre-mutation copy.

**Review fixes** (independent review of the As-built, 2026-09-15). **Not done, on the reviewer's
correct point:** `a_pool_update_mid_round_does_not_change_the_pools_the_round_builds_on` asserts
the carried capture and never drives final construction, so a `fetch_pool_snapshot()` reintroduced
in `try_build_proposal` would pass the suite. Catching it needs `from_proposal` to consult the map,
which needs a `PoolSolution` with at least one swap, and the consensus fixtures cannot build one —
that machinery lives in `crates/types`' own tests. The consume side is therefore held by structure
(one call site, `matching_engine_output`) rather than by a test, the same shape ticket 22's config
test has. Corrections: `matching_engine_output` runs from `ProposalState::new` on the leader *and*
`FinalizationState::new` on verifiers — a node enters exactly one of the two per round, so "once
per round" stands; the verifier path now pays one extra deep copy of the pool map (the carried copy
it never reads), accepted since avoiding it means an `Arc` through `MatchingEngineHandle`. The
capture comment no longer implies block sync synchronises the pools, and `setup_state_machine_with`
derives `tick_spacing` and `fee_in_e6` from the registry key instead of hardcoding them. Rerun:
`cargo nextest run -p consensus --lib rounds` — 13 passed.
