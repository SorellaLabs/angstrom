# 10 — Contract test suite

**Blocks on:** 01

## Files
- `contracts/test/AngstromProtocolFeeConfig.t.sol` (new)
- `contracts/src/periphery/AngstromProtocolFeeConfig.sol` — under test

## Goal
Ship the contract's named tests.

## Do
- `contracts/test/AngstromProtocolFeeConfig.t.sol`.
- Auth: owner, fast owner, everyone else rejected, identical owner and fast owner, reverting
  `owner()` / `fastOwner()` lookups fail closed, and authority following a controller replacement.
- Bounds at `0 / 1_000_000 / 1_000_001`, rejection writes nothing.
- ABI shape: one state-changing function, three views, no fallback, no receive.
- Getter and slot-0 agreement, including a fuzz roundtrip.
- Constructor rejects the zero address and out-of-range initial values.

## Done when
- `forge test --ffi` green; each requirement above fails the suite when violated.
