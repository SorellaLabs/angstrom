Run the existing command:

```sh
cargo run -p protocol-fees -- --ws-url "$ETH_WS_URL"
```

It prints the raw `ControllerV1.collect_unlock_swap_fees` calldata, followed by exact decimal-adjusted token amounts and the numbered/hash-pinned simulation boundary. It never signs or sends a transaction. Bundle-held withdrawals remain zero under `contracts/docs/specific-accounting-protocol-fees.md`.

The recipient and reviewed accounting inputs cannot be inferred from an RPC URL. Put them in `bin/protocol-fees/config.json`, or point `PROTOCOL_FEES_CONFIG` to that file. The original CLI and tracing configuration are unchanged.

The JSON object has three fields:

- `second_ws_url`: an independently operated archive provider with `debug_traceTransaction`/Geth `callTracer` support. Both providers must retain full history from PoolManager deployment.
- `evidence_directory`: a new directory for this run. Existing evidence is never overwritten.
- `review`: the fields of `Review` in `src/live.rs`. Supply the collector, authorized caller, recipient, exact ordered currency list, deployment transaction hashes, reviewed runtime hashes, and current Timelock/Safe configuration. Addresses/hashes/bytes use `0x` hex; raw U256 values use hex quantities.

The review must establish the deployed code and immutable wiring from retained compiler artifacts. Runtime hashes cover the Angstrom, Controller, PoolManager, Collector, owner, fast owner, Safe implementation/extensions, every reviewed token, and recipient. `exact_transfer_tokens` includes historical collection tokens as well as today's selected tokens, with reviewed non-rebasing/exact-transfer behavior. The recipient must be a dedicated account with empty code; its reviewed runtime hash is `keccak256(empty)`. Native currency requires address zero in `assets` and `include_native: true`.

The command scans all collector-involving ERC-6909 transfers, including transfers before the collector existed, and reconciles every discovered ID with both providers. Requested assets containing incidental value, unexpected outflows, incomplete traces, runtime/role changes, or failed simulations stop calldata generation. Remove contaminated assets from the reviewed selection before rerunning; their incidental balance is never counted as protocol revenue.

Evidence files retain the review, paired overlapping log scans, CREATE/outgoing transaction receipts and Alloy traces, raw-unit claim balances, historical burn splits, and final calldata. Supply the calldata to the reviewed Controller from the reviewed caller. Amounts describe the simulation block: collection drains the whole live balance, so incidental claims can arrive afterward. Reconcile the execution receipt and burn prestates before booking revenue, and quarantine any incidental component.

For checks without modifying the root lockfile or allowing the shared build script to regenerate bindings in this checkout:

```sh
python3 bin/protocol-fees/scripts/check.py test -p protocol-fees
```

The script compiles a disposable copy under `bin/protocol-fees/target/` using the repository's existing bindings and crates.
