# 29 — Fix callers broken by the signature change

**Blocks on:** 24

## Files
- `crates/types/src/matching/uniswap/poolsnapshot.rs:226` — `at_tick`
- `crates/uniswap-v4/src/uniswap/pool_providers/mock_block_stream.rs:42`
- `crates/uniswap-v4/src/uniswap/pool_providers/provider_adapter.rs:45`

## Goal
Get `just ci` green.

## Do

1. **Re-verify the premise first.** `cargo check --workspace --all-targets` already exits 0 as of
   ticket 26, so nothing is broken by the signature changes — tickets 21-24 landed their call
   sites with them. Confirm this still holds, then treat the list below as the actual work.

2. Clear the three warnings that fail `cargo clippy --all-targets -- -D warnings`:

- `poolsnapshot.rs:226` — `mismatched_lifetime_syntaxes`. Apply the compiler's suggestion:
  `-> eyre::Result<PoolPrice<'_>>`.
- `mock_block_stream.rs:42` and `provider_adapter.rs:45` — `result_large_err` on a closure. Box the
  error or widen the clippy allow; pick whichever the surrounding code already does.

3. Run `just ci`.

## Done when
- `just ci` builds and passes.

## Notes
**Three of the four items this ticket was written against are already done.** Verified rather than
assumed:

- `crates/angstrom-net/src/manager.rs:303` is **not** a non-exhaustive match on `EthEvent` — it
  ends in `_ => {}` (`:309`), so `ProtocolFeeConfigUpdated` never broke it. The ticket's claim is
  stale.
- Address init for test harnesses is in place: `testing-tools/src/controllers/strom/internals.rs:166`
  and `harness.rs:305` both resolve `PROTOCOL_FEE_CONFIG_ADDRESS` with `.unwrap_or_default()` and
  feed `load_from_chain`, with the zero-address case commented.
- `bin/testnet`, `bin/replay`, the mocks, and the `delta_tps` bench under `crates/matching-engine`
  all compile — `--all-targets` covers benches.

So this reduces to the lint gate. All three warnings are **pre-existing** and unrelated to the fee
work; they are in this ticket because it owns "`just ci` passes" and nothing else does. If they
turn out to predate the branch, fixing them here is still the cheapest place.
