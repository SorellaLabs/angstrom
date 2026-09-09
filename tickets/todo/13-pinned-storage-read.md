# 13 — Read both rates from pinned storage

**Blocks on:** 04, 05

## Files
- `crates/types/primitives/src/contract_payloads/protocol_fees.rs` (new)
- `crates/types/primitives/src/contract_payloads/mod.rs` — module wiring
- `crates/types/primitives/src/primitive/protocol_fees.rs` — `from_slot0`
- `crates/types/constants/src/lib.rs` — config address
- `crates/types/primitives/src/contract_payloads/angstrom/mod.rs:243` —
  `AngstromPoolConfigStore::load_from_chain`, the pattern to follow

## Goal
Storage is the source of truth, read at one block hash.

## Do
- New module `contract_payloads/protocol_fees.rs`, generic over `Provider<N>` and taking the config
  address and a `BlockId`, mirroring `AngstromPoolConfigStore::load_from_chain`.
- Read slot 0 of the config address pinned to a block hash — locally via the provider, or over RPC
  with an EIP-1898 block-hash identifier and `requireCanonical: true`. Never `latest`, never a bare
  number; the existing `load_from_chain` call site passes `Latest`, which is not acceptable here.
- Validate the address holds the expected code for the intended Angstrom immutable before trusting
  the word; an empty account reads as zero storage and must not pass as two valid 0% settings.
- Decode with `DonationSplits::from_slot0`, return a `DonationSplitSnapshot`.

## Done when
- One call returns both rates and the block identity they came from.
- An empty or wrong-code account is an error.

## Notes
Belongs beside the contract payloads it decodes, not in `crates/eth` — that crate consumes
canonical notifications and does no provider fetching.
