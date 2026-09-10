# 35 — Empty-vector and unallocated-budget retention

**Blocks on:** 33

## Files
- `crates/types/src/uni_structure/pool_swap.rs`
- `crates/types/src/traits/bundles.rs` — the `solution.ucp.is_zero()` branch

## Goal
Keep today's behavior, but make it deliberate and accounted.

## Do
- An empty donation vector, and a valid swap whose budget the allocator cannot fully place, leave
  that budget retained as protocol fees through the existing `collect_extra` / `Asset.save` path.
- Make the retention explicit and reported. `Some(empty)` must not skip allocation silently.
- Leave paths that already allocate to LPs alone — this does not route every no-op's budget to the
  protocol.
- A book no-op after a ToB swap uses the post-ToB state.
- A moving swap with missing range metadata is an error, not a no-op.

## Done when
- Retention is reported as its own bucket, not folded into rounding or the configured fee.
- Malformed metadata still fails.

## Notes
Supersedes the earlier requirement that a true no-op allocate its whole budget to the active range
and fail without liquidity. That is withdrawn: no redistribution logic is added.
