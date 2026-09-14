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
