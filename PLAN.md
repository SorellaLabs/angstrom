# `bin/protocol-fees` — what's missing

Target: section 2 of `contracts/docs/specific-accounting-protocol-fees.md`.

The tool computes `savedGross - pulled`. That is an **upper bound**, not protocol revenue: `save`
also holds gas, referral fees, LP-intended residual and rounding dust. It correctly reports zero
collectible and emits no calldata — keep that until every step below is done.

## 1. Get `savedGross` right

- **Discover bundles from Angstrom's anonymous fee-summary log**, not PoolManager `Swap`. The
  current filter keeps `Swap.fee == 0`, but `UnlockHook.sol:62` returns the pool's governance-set
  `unlockedFee` for *every* swap including bundles, so any pool with a nonzero `unlockedFee` has all
  its bundles dropped. Bundles with no swap are dropped too (`PoolUpdates.sol:193` skips when
  `amountIn == 0`). `_nodeBundleLock` (`TopLevelAuth.sol:222`) permits one `execute` per block, so
  one log per block is the complete set.
- **Verify the commitment**: `log.data == keccak256(concat(addr20 || save16))` before accruing.
- **Decode strictly**: full PADE consumption plus a `pade_encode()` round-trip — pade's `Vec`
  decoder silently drops a truncated final asset.

## 2. Get `pulled` right

- **Trace `setController` and `pullFee`.** Neither emits an event (`TopLevelAuth.sol:66,180`), so no
  log filter can find them; this needs `debug_traceTransaction` or per-block storage reads. Reading
  today's `ControllerV1.owner()` `CallExecuted` logs misses pulls through a rotated owner or
  controller, and each miss *inflates* the result.
- **Classify each pull by source.** The code assumes every pull was against `save`, which only holds
  if no collector proceeds were ever sent to Angstrom (`UnlockSwapFeeCollector.sol:40` takes an
  arbitrary `to`). An unclassified pull is a stop.

## 3. Split protocol out of `save`

`save` is fed from four places: ToB gas (`bundles.rs:280,287`), the 25% book-fee complement
(`:509`), and `collect_extra` (`state.rs:180`), which sweeps residual liquidity. User-order gas
arrives *only* through that last channel, mixed with LP-intended residual and dust — so subtracting
`extra_fee_asset0` is not enough; the builder's arithmetic must be replayed bit-exactly (`f64 *
0.75`, saturating) against the exact call-prestate pool config. Require `ref_id == 0` per order or
stop, since `extra_fee_asset0` is gas *plus* referral.

## 4. Prove the remainder is unencumbered

`balanceOf(Angstrom) == U + L + S + P + X`. Nothing here reads `balanceOf`. `U` (deposits,
withdrawals, internal orders) and `L` (LP rewards — currency0 only, including removed pools) emit no
events either, so they carry the same trace-or-replay cost as step 2. `X` must include the
CREATE-prestate balance; Angstrom has no `receive`/`fallback`, so any ETH is always `X`, never a fee.

## 5. Gate it

Verify `eth_chainId`, and end at a fixed block number *and* hash rather than the chain head. Until
every §2.6 gate passes the withdrawable amount is zero by definition; §2.7 calldata follows.



