# 39 — Close the pre-A replay equivalence question

**Blocks on:** —
**Closes:** ISSUES.md 11
**Follows:** 34, 25

## Files
- `crates/types/primitives/src/contract_payloads/protocol_fees.rs` —
  `the_deployed_split_reproduces_the_legacy_f64_path_below_its_bound`, and the `F64_DIVERGENCE`
  const it pins
- `testing-tools/src/replay/runner.rs:277` — the `load_from_chain` call replay already makes
- `bin/replay/src/lib.rs` — the replay entry point

## Goal
Answer whether any pre-**A** block actually diverges, with evidence.

## Do
1. **Answer the magnitude question first — it is the cheap half.** Ticket 34 reduced "would a
   recorded block diverge?" to "did any pre-**A** *per-pool* `total_user_fees` reach
   `3_002_399_751_580_333`?" Query the recorded data for the maximum per-pool `total_user_fees`
   across pre-**A** blocks. If the maximum is below the bound, the question is closed by argument
   and no replay run is needed — record the number and the query.
2. **Only if the maximum reaches the bound**, replay a spread of recorded pre-**A** blocks and diff
   each produced bundle against what was included. That is ticket 34's step 1 as originally written,
   and it needs S3 credentials, recorded block ids, and an archive fork URL.
3. **Only if step 2 shows a real divergence**, build ticket 34's step 2: branch in `process_solution`
   on `snapshot.block_number <= PROTOCOL_FEE_CONFIG_DEPLOYED_BLOCK`. That means threading the
   snapshot rather than just `DonationSplits`, which ticket 23 deliberately decided against — reopen
   23 explicitly rather than widening the signature quietly.

## Done when
- The maximum pre-**A** per-pool `total_user_fees` is a recorded number, not an assumption.
- Ticket 34's first "Done when" bullet — "A block either side of **A** replays identically to what
  was included" — is either satisfied by evidence or superseded by the magnitude argument with the
  number written down.

## Notes
Ticket 34 landed steps 1 (arithmetic half), 3 and 4, and explicitly did not land its empirical half:
"Replaying a spread of recorded blocks needs S3 credentials, recorded block ids, and an archive fork
URL, none of which were available; the first 'Done when' bullet is therefore unverified, and the
magnitude check is the cheap way to close it."

The arithmetic is already pinned and passing.
`the_deployed_split_reproduces_the_legacy_f64_path_below_its_bound` shows `(gross as f64 * 0.75)` and
`split_user(gross).0` agree for every value below `3_002_399_751_580_333`, with the bound tight —
the ten thousand values under it agree and the bound itself differs by exactly one unit. Below it
`3 * fees` fits in f64's 53-bit mantissa.

So this is not a re-derivation, it is a lookup. For an 18-decimal token the bound is 0.003 of it in
fees from a single batch, which is not obviously out of reach — that is why it is a real question
rather than a formality, and also why it is likely to come back "no" and close cheaply.

Step 3 is deliberately last and deliberately conditional. PLAN.md rollout step 6 asks for a legacy
`f64` path; ticket 34's argument is that the baked-in const already satisfies it, because
`load_from_chain` short-circuits to `(750_000, 1_000_000)` at or before the deployed block with no
provider call. The measurement is cheap and the dead branch is forever.

**As built.** Step 1 is answered with a number, and it closes the question: no mainnet bundle was
ever built on a per-pool `total_user_fees` at or above `F64_DIVERGENCE`, so steps 2 and 3 do not
apply and no legacy `f64` branch is built. Ticket 23's decision stays closed.

**The maximum.** The largest per-pool `total_user_fees` in any successful mainnet bundle is
`683_365_780_083_599` wei of WETH — block `23139381`, tx
`0x66423beb49242f88db9cb1f513657b0efcd45c064e614b30c1a7986af97994dc`, pair 0 of the WETH/USDT pool
at a `350` E6 bundle fee — which is `4.39×` below the bound `3_002_399_751_580_333`. The next
largest is `350_459_072_829_326` (block `23139408`); everything else is two or more orders of
magnitude smaller. Only two assets have ever been a token0 on mainnet Angstrom, WETH and USDC, and
the USDC side peaks at `804_062` raw units. Measured 2026-09-14 over blocks `22971781` (Angstrom's
deployment) through `25977666` (the tip at the time), which covers every pre-**A** block and the
29_200 after it. **A** itself is `25948466`.

**How it was measured, so it can be re-run.** "Recorded data" in the ticket's sense (S3 block logs)
was not available, and neither was an archive node; the chain itself was used instead.

1. Every top-level call to mainnet Angstrom (`0x0000000aa232009084Bd71A5797d089AA4Edfad4`) was
   enumerated with `trace_filter` (`toAddress`, 1000-block windows, split on oversize responses)
   against `https://eth.drpc.org`, keeping `traceAddress == []`, no `error`, and selector
   `0x09c5eabe` (`execute(bytes)`): 879_917 successful bundles, 871_164 of them at or before
   **A**+1. The first 10_000 transactions were cross-checked against Blockscout's Etherscan-compatible
   `txlist` for the same range — identical 8_114 successful bundles, zero difference either way.
2. For each bundle the `Asset` array was decoded from the calldata (ABI `bytes` payload, PADE
   3-byte byte-length prefix, 68-byte `addr ++ save ++ take ++ settle` entries) and `save` per
   asset recorded. Pre-activation `save(t0)` is `Σ_pools (fees − ⌊0.75·fees⌋) + swept residuals`,
   so `4·save(t0) ≥ total_user_fees` for every pool on that t0; a bundle can only reach the bound
   if `4·max(save) ≥ F64_DIVERGENCE`. That necessary condition selects 663 candidate bundles
   (660 pre-**A**); the other 879_254 are excluded by the bound alone.
3. Every candidate was fully decoded with the production decoder (`executeCall::abi_decode` then
   `AngstromBundle::pade_decode`) and each pair's `total_user_fees` recomputed exactly as
   `process_solution` / `apply_user_order` compute it: `get_quantities_at_price(!zero_for_one,
   exact_in, fill, extra_fee_asset0, fee_in_e6, Ray(price_1over0)).2` summed over the pair's
   `UserOrder`s, where `fill` is the encoded `Exact { quantity }` or `Partial { filled_quantity }`
   and `extra_fee_asset0` is `priority_data.gas` — the exact value `apply_user_order` used
   (`from_internal_order`, `crates/types/src/traits/user_orders.rs`). `fee_in_e6` is not in the
   bundle; it was read from Angstrom's config store as of each bundle's parent block (slot 3 →
   SSTORE2 store → entry at the pair's `store_index`), because the batch-update path only emits an
   opaque event. It was `350` until the store rotated between parents `23139678` and `23719893`,
   and `200` since. On every one of the 1_256 pool rows the recomputed fees satisfied
   `fees ≤ 4·save(t0)`, and the legacy `(fees as f64 * 0.75) as u128` and the integer
   `fees·750_000/1_000_000` agreed on all of them.

Only 5 of the 1_256 candidate pool rows carry user orders at all (7 orders in total): mainnet
bundles are overwhelmingly ToB-only, and the large `save`s that made them candidates are ToB
allocator remainders, not fees. That is also why the cheap bound alone could not close the question
— the largest WETH `save` is `28_847_913_163_200_000`, 38× over the bound — and why the exact
recomputation was needed.

**What was not done.** No replay run and no bundle diff (ticket 34's step 1 as originally written):
the exact recomputation answers the same question directly and covers every bundle rather than a
spread, but it was validated by source reading of the builder's fee path and by the `fees ≤ 4·save`
identity on every row, not by re-executing a bundle. A random sample of 300 ordinary bundles was
decoded the same way to look for a book-only bundle whose `save` would equal `fees − ⌊0.75·fees⌋`
exactly, but none of the 300 carried a user order, so that end-to-end check has no data point. The
measurement scripts lived in the session's scratch directory and are not checked in; the method above
is what to re-run. Ticket 34's first "Done when" bullet is superseded by this argument with the number
written down, as this ticket's second bullet allows.

**Code change.** One doc-comment update on
`the_deployed_split_reproduces_the_legacy_f64_path_below_its_bound` in
`crates/types/primitives/src/contract_payloads/protocol_fees.rs`, recording the measured maximum so
the next reader of the bound knows it was answered. Verification:
`cargo nextest run -p angstrom-types-primitives the_deployed_split_reproduces` — 1 passed;
`cargo +nightly fmt -p angstrom-types-primitives -- --check` — clean.
