# 32 — Builder bundles against unchanged Angstrom

**Blocks on:** 27

## Overview
PLAN.md's fourth acceptance criterion says hand-written fixtures do not satisfy it, and this is
the ticket that honours that. Stand up the existing Anvil harness against a real deployed
Angstrom, drive `process_solution` and `from_proposal` to produce a genuine `AngstromBundle`,
and submit it. Three things get asserted: that the transaction succeeded — which *is* the
zero-unresolved-delta assertion, since `_saveAndSettle` reverts otherwise — that `save` for
token0 is exactly right, and that reward growth matches the bundle's `RewardsUpdate`. Exact
`save` needs two assertions rather than one because nothing on chain accumulates it; `pullFee`
transfers straight from the raw ERC20 balance. Run it at both the deployed rates and a nonzero
ToB share, so the ToB fee path is exercised somewhere before rollout step 5 turns it on for
real. This is the only test that can catch a `save_amount` that is correct in the builder's
arithmetic but wrong against the contract.

## Files
- `testing-tools/src/contracts/environment/angstrom.rs` — `AngstromEnv`, real deployment
- `testing-tools/src/providers/anvil_submission.rs:27` — `submit`, bundle → signed tx
- `testing-tools/src/types/initial_state.rs` — pool and balance setup
- `crates/types/src/traits/bundles.rs` — code under test

## Goal
Prove settlement against real contracts, not fixtures.

## Do

1. Stand up `AngstromEnv` with a funded pool and orders, drive `process_solution` /
   `from_proposal` to produce a real `AngstromBundle`, and submit it through
   `AnvilSubmissionProvider`. The harness already deploys unchanged Angstrom and applies the
   storage overrides `fetch_needed_overrides` asks for — do not hand-assemble a bundle.

2. **Zero unresolved deltas is the tx succeeding.** `Settlement._saveAndSettle`
   (`contracts/src/modules/Settlement.sol:82`) computes `bundleDeltas.sub(addr, saving + settle)`
   and reverts `BundlDeltaUnresolved(addr)` on any nonzero. Assert the receipt succeeded and say in
   the test *why* that is the delta assertion — a later reader should not re-derive it.

3. **Exact `save` needs two assertions, because there is no on-chain counter.** `pullFee`
   (`TopLevelAuth.sol:180`) transfers from the raw ERC20 balance; nothing accumulates `save` in
   storage. So:
   - decode the submitted bundle's `Asset` array and assert `save` for t0 equals
     `user_protocol_fee + tob_protocol_fee` plus the residuals `collect_extra` swept;
   - assert the Angstrom contract's t0 ERC20 balance grew by exactly that amount across the tx.

4. **Reward growth** — read `poolRewards.globalGrowth` and the per-tick growth
   (`contracts/src/modules/PoolUpdates.sol:61-91`) before and after, and assert the increase matches
   the bundle's `RewardsUpdate`.

5. Run at `(750_000, 1_000_000)` and at a nonzero ToB share, so the ToB fee path is exercised
   somewhere before rollout step 5 turns it on for real.

## Done when
- Hand-written fixtures are not what proves this.
- Exact `save`, a successful settlement, and expected reward growth all assert against a bundle the
  builder produced.

## Notes
This is the ticket that would catch a `save_amount` that is right in the builder's arithmetic but
wrong against the contract — the thing no unit test in `crates/types` can reach, and the reason
PLAN.md calls out fixtures explicitly.

Step 3's balance check is only sound because the harness controls the starting state. It is **not**
a template for anything that reasons about withdrawable amounts on a live chain, where Angstrom's
balance also holds user funds in flight and is explicitly not the withdrawable amount.

`crates/types/tests/angstrom.rs::build_bundle` is the existing fixture test and is `#[ignore]`d with
a stale base64 blob (its `PoolSolution` predates `cancel_requested`). Deleting it once this ticket
lands is reasonable; it is not carrying coverage today.

**As built.** `crates/types/tests/anvil_settlement.rs`, gated `#[cfg(feature = "anvil")]` like the
fixture test it replaces, so it runs under `--features anvil` rather than in `just test-integration`.
One test drives two scenarios: the deployed `(750_000, 1_000_000)` and a nonzero ToB share
`(750_000, 750_000)`.

The bundle is a ToB bid plus a book that clears against itself at the post-ToB price, built through
`for_gas_finalization` — the `process_solution` path that does not need a signed `Proposal` — and
submitted through `AnvilSubmissionProvider`. Each scenario gets its own pool, so every bundle is
built from a snapshot that still matches the chain, and both pools are deployed before the first
submission: the submitter signs with an explicit nonce, which leaves the provider's nonce filler
behind and breaks any later deploy from the same account.

**The env needs a forked chain, and nothing on this branch changed that.**
`deploy_angstrom_create3` mines a hook address through the create3 factory at `SUB_ZERO_FACTORY`,
so that factory has to already exist — which on a bare anvil it does not. The calls to it succeed
against an empty account (a `CALL` to a codeless address returns success) and the function returns
the *computed* address without reading it back, so `AngstromEnv::new` hands out an Angstrom with no
code and the failure only surfaces later as an opaque revert.

This is pre-existing, not a regression: `testing-tools/src/contracts/` is byte-identical to `main`
on this branch, and `SpawnedAnvil::new` had no live caller anywhere in the repo — its only mention
was inside the commented-out `similar_to_prev` block in `crates/types/tests/bundle.rs`. Every anvil
path that was actually exercised either forks (testnet, replay, devnet-with-fork-config) or points
at an external node the developer starts themselves (`LocalAnvil`). This test is the first caller,
which is why the silent-codeless-deploy behaviour had never been hit. `spawn_anvil_forked` / `SpawnedAnvil::new_forked` are added for this, matching what
the testnet configs already do; the fork URL comes from `ETH_WS_URL`, defaulting to the public node
`bin/testnet/tests` already defaults to. `AngstromAddressConfig` is initialized in two stages —
chain id before the node comes up, because `spawn_anvil` reads it, then the deployed addresses so
orders sign against the deployment's own EIP-712 domain.

**Step 3's balance assertion is `save + rewarded`, not `save`.** The ticket says the contract's t0
balance grows by exactly `save`; measured, it grows by `save` plus the donation. LP rewards are
settled as growth, not transferred, so the donated t0 stays in the contract as unclaimed reward.
The identity still pins `save` exactly and is still the second, on-chain half of "exact save" —
`save` alone is simply not the whole of what the contract retains. Both halves are asserted:
`encoded_save == user_protocol_fee + tob_protocol_fee + residual`, where the residual is derived
from the LP budget the splits produced minus what the `RewardsUpdate`s actually placed.

Reward growth is read through `extsload` rather than a binding — there is none for
`poolRewardsGlobalGrowth` — at `keccak256(abi.encode(id, 7)) + REWARD_GROWTH_SIZE`, with the pool
id taken from the Uniswap `PoolKey` Angstrom initializes with (`fee = 0x800000`, the dynamic-fee
flag), not the bundle-fee key. The per-tick half is asserted *unchanged*, which is what a
`CurrentOnly` update should do.

Checked against three mutations: inflating `save_amount`, dropping the reserving `allocate` so
`collect_extra` double counts, and handing the ToB allocator the gross. The first two revert in
`_saveAndSettle` — the failure mode no unit test in `crates/types` can reach — and the third fails
the builder's own conservation check.

`crates/types/tests/angstrom.rs` and its `solutionlib` fixture are deleted, per this ticket's note:
`build_bundle` was `#[ignore]`d on a stale base64 blob and carried no coverage. `solutionlib` had
no other consumer.
