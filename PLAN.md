# `bin/protocol-fees` — what's missing

Target: section 2 of `contracts/docs/specific-accounting-protocol-fees.md`.

The tool computes `savedGross - pulled`. That is an **upper bound**, not a protocol-owned amount: it
still contains gas fees, referral fees, LP-intended residual and incidental value. It correctly
reports zero collectible and emits no calldata — keep that until everything below is done.

## Fix in the current scan

- **Find bundles by Angstrom's fee-summary log, not PoolManager `Swap`.** `PoolUpdates.sol:193` skips
  the swap when `amountIn == 0`, but `Angstrom.sol:74` still commits `save`. Book-only bundles are
  silently dropped today.
- **Verify the commitment.** Require `log.data == keccak256(concat(address20 || save16))` before
  accruing any `save`. Nothing currently ties the decoded values to the chain.
- **Decode strictly.** Require full PADE payload consumption plus a `pade_encode()` round-trip —
  pade's `Vec` decoder silently drops a truncated final asset.
- **Trace `setController` and `pullFee` directly.** Reading only today's `ControllerV1.owner()`
  `CallExecuted` logs misses pulls through a rotated owner or controller. Each missed pull *inflates*
  the result.
- **Pin the measurement.** Verify `eth_chainId` before using mainnet addresses, and end at a fixed
  block number *and hash* rather than the chain head.

## Build from scratch

- **§2.4 — separate protocol from everyone else.** `save` is gas + referral + book fee + builder
  residual, commingled. Needs a governance-approved ownership rule and a bit-exact replay of the
  historical builder. The decoded bundle already carries `top_of_block_orders` and `user_orders`;
  both are currently discarded.
- **§2.5 — prove the money is there.** `balanceOf(Angstrom) == U + L + S + P + X`. Nothing here reads
  `balanceOf`. Needs internal user balances, LP allocations minus payouts, collector proceeds, and
  provenance-tracked incidental surplus.
- **§2.6 — gate the withdrawal.** Until every gate passes, the withdrawable amount is zero by
  definition. §2.7 calldata and §2.8 reconciliation follow.
