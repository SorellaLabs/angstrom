This command covers bundle-held savings only (section 2 of
`contracts/docs/specific-accounting-protocol-fees.md`). It does not collect unlocked-swap fees.

```sh
cargo run -p protocol-fees -- --ws-url wss://your-archive-node --recipient 0xYourRecipientAddress
```

Use a mainnet RPC node with historical logs, transactions, and token metadata.
The command uses the repository's address helpers, generated contract bindings, and `AngstromBundle`
PADE decoder. Construction captures the current block as `max_block`. The calculation scans
fee-summary logs after the last successful nonzero `distributeFees` call through that fixed block,
or from Angstrom deployment if no collection exists. Timelock `CallExecuted` logs identify successful
collections; the log position preserves bundles later in the same block. It fetches bundle
transactions and decodes their calldata to match each bundle's savings commitment. Token
amounts are displayed with their on-chain decimals. Inputs and results stay in memory; there are
no configuration files, environment-variable overrides, journals, or transaction submissions.

The reported savings cover this interval. A previous partial or token-specific distribution does
not prove that all earlier savings were collected.

**Gross savings are not collectible protocol fees.** The specification's current audit authorizes
zero bundle-held withdrawals. The output therefore reports observed gross savings, zero collectible
fees, and no collection calldata. It does not encode an empty transaction or treat `save`, token
balances, or a nominal percentage as an authorized withdrawal.

The single-provider scan is a reconstruction aid, not the complete section-2 accounting proof.
It decodes transaction calldata directly and does not use debug tracing. Bundles routed through
a different outer call cannot be decoded by this command and stop the calculation.
Nonzero `ControllerV1.distributeFees` calldata still requires independently verified history,
deployment/runtime and builder provenance, an approved ownership rule, labeled previous claims,
reconciled user/LP/incidental balances, active reservations, and a successful simulation. Those
inputs cannot be inferred from an RPC URL and recipient alone.
