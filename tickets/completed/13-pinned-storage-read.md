# 13 — Load the config from chain

**Blocks on:** 04, 05, 12

## Files
- `crates/types/primitives/src/contract_payloads/protocol_fees.rs` — the types, the const, and
  `load_from_chain`
- `crates/types/primitives/src/contract_payloads/mod.rs` — module wiring
- `crates/types/primitives/src/primitive/mod.rs` — drop the `protocol_fees` module
- `crates/types/constants/src/lib.rs` — `PROTOCOL_FEE_CONFIG_ADDRESS`,
  `PROTOCOL_FEE_CONFIG_DEPLOYED_BLOCK`
- `crates/types/primitives/src/contract_payloads/angstrom/mod.rs:243` —
  `AngstromPoolConfigStore::load_from_chain`, the pattern to follow

## Goal
One module owns the config types and the read. Blocks at or before deployment resolve without
touching the chain.

## Do
- Move `DonationSplits` and `DonationSplitSnapshot`, with their impls and tests, into
  `contract_payloads/protocol_fees.rs`. Delete `primitive/protocol_fees.rs` and its `pub mod` /
  `pub use` in `primitive/mod.rs`. Fix importers.
- Add a module-private const holding the values the contract is deployed with:

```rust
const DEPLOYED_INITIAL_PROTOCOL_FEE_CONFIG: DonationSplitSnapshot = /* 750_000, 1_000_000 */;
```

- Delete `DonationSplitSnapshot::deployed_initial()`; the const replaces it.
- `load_from_chain` returns `eyre::Result<DonationSplitSnapshot>` and short-circuits before any
  provider call:

```rust
if block_number <= PROTOCOL_FEE_CONFIG_DEPLOYED_BLOCK {
    return Ok(DEPLOYED_INITIAL_PROTOCOL_FEE_CONFIG);
}
```

- Otherwise read pinned to the requested block: `get_code_at` must be non-empty, `angstrom()` must
  equal `ANGSTROM_ADDRESS`, then slot 0 (`PROTOCOL_FEE_CONFIG_SLOT`, named beside
  `CONFIG_STORE_SLOT`) decoded with `DonationSplits::from_slot0`.

## Done when
- Nothing outside this module can construct the initial config.
- A block at or before the deployed block returns the const and makes no provider call.
- A block after it reads chain state; an empty account, wrong code, or a deployment bound to a
  different Angstrom is an error.

## Notes
`PROTOCOL_FEE_CONFIG_DEPLOYED_BLOCK` is a `OnceLock`, so it cannot be read in const context — the
const's own fields are literals.

The const bypasses `DonationSplits::new`, the documented only constructor. The values are literal
and in range by inspection; keep them next to `DENOM` so a change is visible.

The const carries a fixed block identity, so a pre-deployment resolution is distinguishable from a
chain read. Consumers that check parent identity (23, 37) must expect that.

Pre-deployment resolution is what lets replay work at or before **A** without the caller
special-casing it — see ticket 43.
