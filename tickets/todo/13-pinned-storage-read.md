# 13 — Load the initial config from chain

**Blocks on:** 04, 05

## Files
- `crates/types/primitives/src/contract_payloads/protocol_fees.rs` (new)
- `crates/types/primitives/src/contract_payloads/mod.rs` — module wiring
- `crates/types/primitives/src/primitive/protocol_fees.rs` — `DonationSplits::from_slot0`
- `crates/types/primitives/src/contract_payloads/angstrom/mod.rs:243` —
  `AngstromPoolConfigStore::load_from_chain`, the pattern to follow

## Goal
One pinned read that gives the node its starting rates. Everything after this comes from logs.

## Do
- Mirror `AngstromPoolConfigStore::load_from_chain`:

```rust
pub async fn load_from_chain<N, P>(
    config_address: Address,
    block_id: BlockId,
    provider: &P
) -> eyre::Result<DonationSplits>
where
    N: Network,
    P: Provider<N>
```

- `get_code_at(config_address).block_id(block_id)` must be non-empty. An empty account reads as
  zero storage and would otherwise decode as two valid 0% settings.
- Confirm the deployment is bound to this node's Angstrom: call `angstrom()` at the same block and
  compare against `ANGSTROM_ADDRESS`. The wrong deployment silently yields someone else's rates.
- `get_storage_at(config_address, PROTOCOL_FEE_CONFIG_SLOT).block_id(block_id)` — slot `0`, named
  as a constant in this module following `CONFIG_STORE_SLOT` in `contract_payloads/mod.rs`.
- Decode with `DonationSplits::from_slot0`. Errors are `eyre::Result`, not `String`.

## Done when
- One call returns both rates from one block.
- An empty account, a wrong-code account, or a deployment bound to a different Angstrom is an
  error.

## Notes
Called once, at node init (ticket 16). There is no per-block provider read: the round path never
touches a provider for this, so solving stays off the network. Belongs beside the contract payload
it decodes, not in `crates/eth`.
