# 32 — Fix callers broken by the signature change

**Blocks on:** 27

## Files
- `crates/angstrom-net/src/manager.rs:303` — non-exhaustive `EthEvent` match
- `testing-tools/src/mocks/`, `testing-tools/src/types/initial_state.rs` — address init
- `bin/testnet/`, `bin/replay/`
- benchmarks under `crates/matching-engine/`

## Goal
Get the workspace compiling again.

## Do
- Mocks, benchmarks, testnet setup, and replay call sites.
- Address init for test harnesses in `testing-tools`.
- `angstrom-network` also has a non-exhaustive match on `EthEvent` since
  `ProtocolFeeConfigUpdated` was added.

## Done when
- `just ci` builds and passes.
