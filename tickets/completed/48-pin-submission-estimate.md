# 48 — Pin submission-time gas estimation to the construction parent

**Blocks on:** —
**Closes:** ISSUES.md 5 (PR #680 A.5)
**Follows:** — (the code is `main`'s; PLAN.md assigns the fix to this work)

## Files
- `crates/types/src/submission/mod.rs:186-230` — `submit_tx`: nonce by number, fees, chain id, the
  `bundle_gas_used` closure with bare `estimate_gas(tx).await.unwrap()`
- `crates/types/src/submission/mempool.rs:54-60` — `build_and_sign_tx_with_gas`, the consumer
- `crates/consensus/src/rounds/proposal.rs:125-135` — the call site, which has
  `handles.block_height` in scope and passes only `target_block`

## Goal
Submission preparation reads the same parent state as construction did, executes in an H+1
environment, and cannot panic.

## Do
- Add the construction parent (`BlockNumHash`) to `submit_tx`'s signature and pass
  `handles.block_height` from `try_build_proposal`. `target_block` is `H + 1` and stays.
- Pin the nonce lookup to the parent *hash*, not `target_block - 1` by number — a number cannot
  name one branch of a same-height reorg.
- Pin `estimate_gas` to the parent hash and give it the H+1 block environment (number, timestamp,
  base fee), so it simulates the same thing bundle validation simulated. Alloy's default for a bare
  `estimate_gas` is `pending`, which is neither.
- Replace the `.unwrap()` with `?`. An estimation failure is a failed submission, not a panic in a
  spawned task.

## Done when
- The nonce and the gas estimate are both resolved against the construction parent's hash.
- The gas estimate runs in an H+1 environment.
- A provider error during estimation surfaces as `Err` from `submit_tx` and is recorded by the
  existing submission metrics, and the task does not panic.
- A submission whose parent is no longer canonical at estimation time is observable as such.

## Notes
PLAN.md, Round semantics: "Submission-time `estimate_gas` needs the same parent state and H+1
environment." `crates/types/src/submission/mod.rs` is untouched by this branch; the reviewer notes
the same ("This code predates the PR but remains an explicit, unimplemented handoff requirement").

Today `:196-198` pins the nonce by number and `:212` does not pin the estimate at all. A reorg or
head advance between construction and submission therefore changes the state used to prepare the
transaction, and the gas limit actually sent is derived from whatever the endpoint considers current.

This is the ticket that gets the parent into the submission path; tickets 44 (identity re-check
before signing and each send) and 47 (record the construction parent) both consume what this
threads. Land this first.

Bundle validation's `simulate_bundle` already builds the H+1 environment from the parent
(`crates/validation/src/bundle/mod.rs`); reuse its shape rather than inventing a second one.

**As built.** `submit_tx` takes the construction parent — `submit_tx(signer, bundle, parent:
BlockNumHash, cancel)` — and derives `target_block = parent.number + 1` from it, so the two can no
longer disagree; `try_build_proposal` passes `handles.block_height`. The nonce is read with
`get_transaction_count(from).hash(parent.hash)`. In production that read is served by
`RethDbLayer` from the local reth database, whose `provider_at(BlockId::Hash)` already resolves by
hash; its two `unwrap`s are now errors, so a parent the node cannot resolve — reorged out between
construction and submission — surfaces as `Err` from `submit_tx` instead of a panic on the task.
That is also what makes "no longer canonical at estimation time" observable: the nonce read or the
simulation fails, the future logs `submission failed` and `record_submission_completed(.., false)`.

**The estimate is `eth_simulateV1`, not `eth_estimateGas`, and that is forced.** reth v2.0.0's
`eth_estimateGas` takes `(request, block, stateOverride)` and no block overrides — alloy's
`with_block_overrides` would be sent as a fourth positional parameter that jsonrpsee silently
ignores — so a hash-pinned estimate runs in the *parent's own* environment. There Angstrom's
`_nodeBundleLock` reverts `OnlyOncePerBlock` whenever a bundle landed in H, and flash orders carry
the wrong `validForBlock`. `eth_simulateV1` pinned to `parent.hash` executes on H's post-state in
the environment reth derives for the next block (`next_evm_env(&parent)`: number H+1, timestamp
H+12), the same shape `simulate_bundle` uses, with `validation: false` matching its
`disable_nonce_check`. Verified against mainnet on 2026-09-14 with the public node: the bundle
`0xdb0a4ac0b3119684839b83547abc8561f97dbe630951b8ba5b0a8480ff6c079a` (block 25978416) simulated
at its parent `0x548f809f…be94e` reports simulated block number `25978416`, `status: true` and
`gasUsed: 155858` — exactly the receipt's — while `eth_estimateGas` for the same request pinned to
the same parent hash reverts (`panic: arithmetic underflow or overflow (0x11)`). Anvil 1.6/1.7
serves `eth_simulateV1` too, which is what the harness's `AnvilSubmissionProvider` reaches through
the same closure.

**Gas limit unchanged in shape.** `bundle_gas_used` returns simulate's `gasUsed + EXTRA_GAS_LIMIT`
and every submitter still adds `EXTRA_GAS_LIMIT` again, as on `main` — four recent mainnet bundles
show `gas limit − gasUsed ≈ 206k`, i.e. `eth_estimateGas` sat only ~6k above real usage, so the
on-chain limit moves by about that much on a 200k margin. The estimate stays lazy in the submitters
rather than eager in `submit_tx`: the anvil harness writes its balance overrides inside `submit`,
before signing, and an eager estimate would run without them.

**Errors, not panics.** `bundle_gas_used` is `eyre::Result<u64>`, `build_and_sign_tx_with_gas` is
fallible (its `.build(signer).await.unwrap()` is `?` too, as is the angstrom submitter's inline
copy), and each submitter propagates. `submit_tx` used to drop submitter errors silently; it now
logs them and returns `Err` when *no* submitter produced a result — the estimate is shared by all
of them, so an estimation failure is exactly that case — and `Ok(results)` otherwise, so one
endpoint's failure still cannot hide the others' results. The proposal future's existing `Err` arm
records the failed submission in the metrics.

Not done: no automated test of the RPC shape (alloy's mocked transport answers calls but does not
expose the block id they were made with); it is verified by hand above, and `a_reset_round_is_not_signed`
in `crates/types/src/submission/mod.rs` drives the now-fallible signing path. `Cargo.lock` grew
`tokio-util` under `angstrom-types` for the token ticket 44 puts in the same signature.

Verification: `cargo nextest run -p angstrom-types --lib submission` — 1 passed; `cargo nextest run
-p consensus --lib rounds` — 13 passed (the submission path over the mocked node); clippy and fmt
as recorded on ticket 44. The mainnet `eth_simulateV1` / `eth_estimateGas` comparison above is the
behavioural check; it was run with `cast rpc` against `https://ethereum-rpc.publicnode.com` and
against a local `anvil` (1.6.0-v1.7.0), which answers `eth_simulateV1` as well.

**Review fixes** (independent review of the As-built, 2026-09-15) — three real defects in the
first As-built, each verified before fixing.

1. **`eth_simulateV1` still charges gas up front.** With `validation: false` reth disables the
   nonce and base-fee checks but not fee charging, and a call with no `gas` gets the default limit
   (min of the block gas limit and the 50M RPC cap); anvil behaves the same. So the node account
   had to hold tens of millions of gas × max fee in ETH or the whole RPC errored, before signing,
   for every submitter — where `eth_estimateGas` disables fee charging, which is why `main` never
   hit it, and the mainnet check above replayed a mined transaction that carried its own `gas`.
   The sender now gets a fake balance through `stateOverrides`. Verified on anvil — an unfunded
   sender fails without it (`Insufficient funds for gas * price + value`) and succeeds with it — and
   on mainnet, where the same bundle at the same parent still reports block `25978416`, success and
   `155858` gas with the override in place.
2. **The H+1 environment was implicit.** reth derives the next block's environment for a pinned
   simulate; anvil does not — pinned to the latest hash it simulates *in* that block (latest `0`
   simulated as `0`, as `1` only with an override; the reviewer showed a `block.number`-gated call
   failing there). That is the environment the testnet and replay harness reach through
   `AnvilSubmissionProvider`, so every bundle carrying a ToB or flash order would have failed
   estimation there. `blockOverrides.number = parent.number + 1` is now explicit; reth accepts it
   (it rejects only a number at or below the parent's) and mainnet returns the same result with it.
3. **The anvil-gated acceptance fixture did not compile** — see ticket 44's Review fixes.

Also: the limit base is `gas_used + gas_used / 4` before the margins — simulate reports gas net of
refunds and EIP-3529 caps refunds at a fifth of pre-refund usage, so a quarter over bounds it —
where `eth_estimateGas`'s minimal successful limit already included refunds. `build_and_sign_unlock`
is fallible too, which removes the last `unwrap` on the signing path. Correction to the ticket's
premise: alloy sends no block for a bare `estimate_gas`; the endpoint decides (reth: `latest`, the
parent's own environment; anvil: `pending`, H+1), which is why the harness path on `main` ran at
H+1. Reruns after the fixes: `cargo check -p angstrom-types --features anvil --tests` — clean;
`cargo nextest run -p angstrom-types --lib submission` — 1 passed; `cargo nextest run -p consensus
--lib rounds` — 13 passed; clippy and the anvil settlement run recorded below.

Post-review verification: `cargo clippy` on the seven touched crates with the two standing allows,
with and without `--features anvil` — clean; `cargo +nightly fmt --check` — clean;
`cargo nextest run -p angstrom-types --features anvil --test anvil_settlement` against the public
fork — 1 passed (three scenarios, `builder_bundles_settle_against_unchanged_angstrom`), so the
acceptance-criterion-4 test runs again and the harness's `AnvilSubmissionProvider` settles real
bundles through the new `TxFeatureInfo`.
