# ISSUES — `feat/on-chain-protocol-fee-state` against PLAN.md and PR #680

**Scope: every defect that stands between this branch and PLAN.md, whether the code is new here or
inherited from `main`.** An earlier revision of this file excluded `main`-rooted items; that scoping
is withdrawn — this PR is to cover them. Where an issue's code is unchanged from `main` it says so,
because that changes how it is fixed, not whether.

**Sources.**
- Local review of the full branch diff against the merge base `3690f919` (97 files, ~6.3k insertions)
  plus all 36 tickets in `tickets/completed/`.
- PR #680 (https://github.com/SorellaLabs/angstrom/pull/680): one `COMMENTED` review (2026-09-10,
  contract only, on `77263de9`), one `CHANGES_REQUESTED` review (2026-09-14, ten spec findings and one
  standards note, on `209bc2e4`), and eight inline comments (2026-09-10), all by 0xvanbeethoven. No
  discussion comments, no replies. The 2026-09-14 review reviewed against "Handoff 57ztqhocj3jv
  revision 8" and explicitly excluded this repo's `PLAN.md`, `ISSUES.md` and `tickets/`, so its
  agreement with the local review is independent confirmation.

Every claim below, the reviewer's and mine, was checked at `209bc2e4` — exactly the PR head the
latest review was written on. The tree has since advanced one commit to `61af55f3`, which lands
tickets 40 and 41; the two issues they closed (`DonationResidual` mis-attribution on the
both-vectors-`None` path, and its unchecked `total()`) are fixed and no longer listed. PR findings
are tagged
`[PR A.n]` (spec findings, in the review's order), `[PR std]` (standards note), `[PR C.n]` (inline
comments, in creation order).

## Verified clean

Checked directly, not inferred:

- `cargo check --workspace --all-targets` — clean.
- `forge test --match-contract AngstromProtocolFeeConfig --ffi` — 29 passed, 0 failed.
- `cargo test -p angstrom-types-primitives --lib protocol_fees` — 17 passed, 0 failed. (The reviewer
  additionally reports 45/45 `angstrom-types` and 23/23 `angstrom-eth` lib tests and the
  feature-enabled anvil settlement test passing; my `angstrom-types --lib` run was killed by memory
  pressure twice, so that count is taken on trust.)
- `forge inspect AngstromProtocolFeeConfig storageLayout` — `_userLpShareE6` slot 0 offset 0,
  `_tobLpShareE6` slot 0 offset 4, immutable takes no slot. Matches PLAN.md's decoder contract.
- Selectors match PLAN.md exactly: `setLpDonationSplits` `0xb3226f60`, `getLpDonationSplits`
  `0xfa35d883`, `angstrom` `0xff3ddeb8`, `controller` `0xf77c4791`.
- ABI shape: one state-changing function, three views, no fallback, no receive, no payable.
- The contract source matches PLAN.md's specified contract line for line.
- The reviewer's 2026-09-10 contract verdict — "no correctness or security defect in the contract;
  it is safe to deploy on mainnet" — agrees on every item I could check (layout, no value path,
  bounds, atomic pair write, event on every change, 29/29 tests). Its mainnet dry-run details
  (controller `0x1746…fD4`, owner = TimelockController with 5-day delay, fastOwner = Safe threshold
  2, `verify()` passing) need RPC to confirm and match what the deploy script asserts.
- The nine unrelated `abis-types/*.json` artifacts this branch rewrote are **semantically
  identical** to `main` after normalising key order — reformatting only, no ABI drifted.
- Integer split arithmetic, slot-0 decode, the `serde` bounds guard, the config read's
  code/`angstrom()` validation, log application across every block of a notification, reorg
  inversion from the event's `old*` pair, per-pool application of both splits, the conservation
  checks, and `save_amount = user_protocol_fee + tob_protocol_fee` all match PLAN.md and are
  covered by named tests.
- PR inline comments **C.7** (`require(fastOwner != 0)` too strict for testnet) and **C.8**
  (`.expect` panics every node with zero constants) are **already fixed at head**: the require is
  gated to mainnet (`script:85-89`) and `components.rs:315-323` uses `.map_err(..)?` with real
  constants set. Both comments are anchored on superseded commits. Nothing to do.

Issues are ordered by severity. Each names the PR finding it corresponds to, the completed tickets
that own the code, and the `tickets/todo/` ticket that closes it.

---

## 1. Round reset does not abort the submission task `[PR A.2]`

*Owned by nothing on this branch — the code is `main`'s. Closed by ticket 44 (with acceptance
criterion 2's test).*

**PLAN.md, Round semantics:** "Round reset must **abort** its submission task, not just drop the
join handle — a dropped handle leaves the task running. Re-check cancellation and identity after
async preparation, before signing, and before each endpoint send." **Acceptance criterion 2:**
"Invalidate a round while matching, simulation, signing, or an endpoint send is in flight; assert
no later send or retry occurs. Assert on sends that did not happen, not on the presence of a
token. Cover the dropped-join-handle case."

**Verified.**
- `crates/consensus/src/rounds/proposal.rs:320`:
  `self.submission_future = Some(Box::pin(tokio::spawn(submission_future)))`, field type
  `Option<BoxFuture<'static, Result<bool, JoinError>>>` (`:36`) — a boxed `JoinHandle`.
- `crates/consensus/src/rounds/mod.rs:142` `reset_round` replaces `self.current_state`, dropping the
  `ProposalState` and that handle. Dropping a `JoinHandle` detaches the task; it does not abort it.
- `crates/types/src/submission/mempool.rs:54-76`: `submit` awaits `build_and_sign_tx_with_gas`
  (nonce, estimate, sign) and then fans `send_raw_transaction` out to every client. No check of any
  kind between preparation and the sends. `grep -rniE "cancel|abort|generation" crates/consensus/src`
  finds nothing.

So a reset that lands during nonce lookup, estimation or signing still produces endpoint sends for
the invalidated round. The reviewer's framing is exact: these are sends *not yet made* at reset
time, outside PLAN.md's accepted "already submitted" limitation. No test covers any of it.

**Status.** `proposal.rs:314` on `main` is the identical `tokio::spawn` with the identical field
type. This is a live correctness bug on `main` that PLAN.md assigned to this work and the branch did
not touch.

---

## 2. Startup drops queued notifications, and the config is never reconciled with storage `[PR A.4]`

*Owned by tickets 07, 13, 14, 15, 16 — closed by ticket 37.*

**PLAN.md, Reading canonical state, item 3:** "On commit and on reorg, **read the new head** and
publish with that block's identity. A reorg that merely removes an update carries no replacement
event, **which is why storage is the source of truth.** Index `LpDonationSplitsSet` separately for
operator-facing change history and telemetry [...] It is a view over what storage already decided,
**never a second source of configuration.**" **Item 1:** "Subscribe to canonical updates before
taking the startup snapshot, then reconcile queued updates."

The implementation inverts item 3. Storage is read exactly once, at init
(`bin/angstrom/src/components.rs:315`). From then on the pair is maintained purely by decoding
`LpDonationSplitsSet` logs (`crates/eth/src/manager.rs:230-234, 309-317`), and reorgs are inverted
from the removed event's `oldUserLpShareE6` / `oldTobLpShareE6` pair
(`crates/eth/src/manager.rs:384`). Tickets 07, 14 and 15 chose this deliberately and argued it
well — the setter writes the full pair and the event records both sides, so a removed change *is*
invertible from the log alone — and the inversion logic is correct and has four named tests. It is a
reasoned deviation. What it gives up is self-healing: nothing ever re-reads storage, so a pair that
drifts stays drifted and nothing notices.

**The startup path makes drift certain, not merely possible.** Verified in
`bin/angstrom/src/components.rs:266-315`:
- `:266` `let mut sub = subscribe_to_canonical_state()`; `:268` one `recv()`.
- `:286-291` `fetch_angstrom_pools(deploy_block, ..)` scans from deployment — the code's own comment
  at `:303-305` says "the fetch pools takes awhile" and admits a gap. Every block that lands during
  the scan queues on `sub`.
- `:307` `sub.recv()` — a `tokio::broadcast::Receiver` returns the *oldest* queued message, so this
  selects the first block after the scan began, not the tip. Call it H.
- `:313` `eth_data_sub = subscribe_to_canonical_state()` — a fresh receiver sees only messages
  published after it is created. Everything still queued on `sub` after H is dropped with `sub`.
- `:315` `load_from_chain` at H. The cleanser then only ever applies logs.

A setter in any of H+1 .. tip is in neither the H storage read nor the cleanser's stream, and no
later block repairs it. The reviewer reproduced this against the real Tokio broadcast channel:
selected parent 101, missed setter 102, first fresh notification 103. It is deterministic on every
cold start whose pool discovery spans two or more blocks.

Two further drift paths, each silent:
- **Missed receipts.** `logs_in_block_order` uses `receipts_by_block_hash(...).unwrap_or_default()`
  (`crates/eth/src/manager.rs:406`). A block whose receipts do not resolve is skipped.
- **Reorg without the setter in `old`.** If a reorg notification's `old` chain does not carry the
  `LpDonationSplitsSet` being reverted, `reverted_protocol_fee_config` returns `None` and the node
  keeps the reorged-out pair.

At the shipping rates `(750_000, 1_000_000)` the blast radius is zero because the config never
changes. It becomes live the moment rollout step 5 uses the setter.

**Status.** The config mechanism is new on this branch; the subscription plumbing is `main`'s. Fix
both halves: keep and drain the original subscription at startup, and reconcile the log-derived pair
against slot 0 pinned to each canonical head, treating disagreement as an error.

---

## 3. Simulation's parent pin does not hold — the selector is shared and mutable `[PR A.1]`

*Owned by tickets 19, 20, 21 — closed by ticket 43.*

**PLAN.md, Round semantics:** "Simulation must be pinned to the requested parent hash for every
read including cache misses [...] What that needs is **an immutable provider per parent hash**,
caches that never carry state across hashes, and unavailable state surfacing as an error."

**Verified.**
- `crates/validation/src/lib.rs:104-124`: one `revm_lru = Arc::new(db)` is handed to
  `SimValidation::new(revm_lru.clone(), ..)` (order validation), `FetchUtils::new(.., revm_lru.clone())`,
  and `BundleValidator::new(revm_lru.clone(), ..)`. All three share one wrapper.
- `crates/types/src/reth_db_wrapper.rs:34` `block: Arc<RwLock<BlockNumHash>>`; `:38` `set_block(&self)`
  moves it for every clone; `:59-61` `state()` reads `self.block.read().hash` on every call, so every
  cache miss re-resolves the live selector.
- `crates/validation/src/bundle/mod.rs:125`: `self.db.db.set_block(parent)`, *then* `self.db.clone()`,
  *then* `thread_pool.spawn_raw(..)`.
- `crates/validation/src/validator.rs:148`: `ValidationRequest::NewBlock` calls `set_block` on the
  same wrapper.

Both directions hold: a `NewBlock` moves an in-flight bundle simulation forward to the new head, and
a queued historical simulation moves live order validation backward. The reviewer reproduced the
clone interference in both directions against the actual `RethDbWrapper` (selector proof, not a full
EVM race — which is honest and sufficient). The pin holds at setup time only, and the parent stamped
on the result (`BundleGasDetails::parent`) does not prove the reads used it.

Also on this branch, same site: `validator.rs:146` resolves the hash with
`self.db.block_hash(block_number).unwrap().unwrap()` — two panics on a path PLAN.md asks to be
error-surfacing. `main` had no lookup here.

**Status.** `main` already has `trait SetBlock { fn set_block(&self, block: u64) }` and
`block: Arc<AtomicU64>` shared by every clone. This branch improved it substantially — hash-addressed
selector (19), every read through one `state()` that errors instead of answering from the tip (20),
and `simulate_bundle` pins at all, which it did not before (21) — and left the sharing, which ticket
19 records as its two deliberately unmet "Done when" clauses. Resolve an immutable provider per
simulation with caches bound to it; never set the live validator's selector to serve a historical
bundle.

---

## 4. Final construction re-fetches mutable pool state `[PR A.3]`

*Owned by tickets 22, 23 — closed by ticket 45.*

**PLAN.md, Round semantics:** "Retain the round's pool snapshots too, rather than re-fetching
mutable pool state for final construction." **Acceptance criterion 1:** "assert gas estimation and
final construction used the same snapshot and parent hash, and that neither re-read config *or pool
state*."

**Verified.**
- `crates/consensus/src/rounds/mod.rs:363` — `fetch_pool_snapshot()` for matching and gas
  estimation, captured alongside `splits` and carried out on `MatchingOutput`.
- `crates/consensus/src/rounds/proposal.rs:160` — `handles.fetch_pool_snapshot()` **again**, and that
  second read is what `from_proposal` builds the final bundle from.
- `fetch_pool_snapshot` (`rounds/mod.rs:288-304`) reads live `SyncedUniswapPools`, which is written
  under a lock from the uniswap pool manager's own task: `pool_update_workaround` on each block and
  `load_more_ticks` on tick loads (`crates/uniswap-v4/src/uniswap/pool_manager.rs:303,317`). The
  latter is driven by `calculate_rewards`, which order validation calls for every incoming ToB order
  (`crates/validation/src/order/state/mod.rs:155`), so it fires on ordinary order arrival throughout
  the round.
- `crates/consensus/src/manager.rs:330-352`: `poll` drains `canonical_block_stream` (which resets
  rounds) *before* polling `consensus_round_state`, but the pool manager writes independently. So
  within one poll, `try_build_proposal` can read post-block pools while the pre-block round is still
  current — the reviewer's mechanism exactly.

The config half of this requirement landed (the snapshot rides on `MatchingOutput`); the pool half
was never started. The signed bundle can be priced against different AMM state than its gas estimate
and solutions.

**Status.** Both `fetch_pool_snapshot()` calls are on `main` (`proposal.rs:161` there). Fix is the
same shape as the config fix: carry `pool_snapshots` on `MatchingOutput` and use them in
`try_build_proposal`.

---

## 5. Submission-time `estimate_gas` is not pinned to the construction parent `[PR A.5]`

*Owned by nothing on this branch — the code is `main`'s. Closed by ticket 48.*

**PLAN.md, Round semantics:** "Submission-time `estimate_gas` needs the same parent state and H+1
environment."

**Verified.** `crates/types/src/submission/mod.rs:196-198` `get_transaction_count(from).number(target_block - 1)`
(pinned by number, which cannot name one branch of a same-height reorg); `:212`
`node_provider.estimate_gas(tx).await.unwrap() + EXTRA_GAS_LIMIT` — no block id (Alloy defaults to
pending), no H+1 environment, and a provider error panics inside the submission future. The
submission API receives `target_block` and no parent hash at all.

**Status.** `crates/types/src/submission/mod.rs` is untouched by this branch; the reviewer notes the
same. Thread the construction parent into submission, pin the estimate to it with an H+1
environment, and propagate failures.

---

## 6. The returned parent identity is never checked, and neither parent is recorded

*Owned by ticket 21 — closed by ticket 46 (check + acceptance criterion 1's test) and ticket 47
(record).*

**PLAN.md, Round semantics:** "Identify async work by parent hash plus a round generation that
changes on reset, and **discard results that no longer match** — a matching block height is not
enough, since same-height reorgs exist." **Accepted limitation:** "**Record the construction parent
and the actual inclusion parent so the mismatch is visible afterwards.** Do not claim stale bundles
are excluded." **Acceptance criterion 1, second half:** "Change the head mid-round — including a
same-height reorg — and assert the stale result is rejected."

**Verified.**
- Ticket 21 added `parent: BlockNumHash` to `BundleGasDetails` with an accessor
  (`crates/types/primitives/src/contract_payloads/angstrom/mod.rs:141`). The accessor has **no
  callers**; `from_proposal` takes the value as `_gas_details` (`crates/types/src/traits/bundles.rs:848`)
  and the struct keeps `#[allow(unused)]`. So the identity is carried and dropped.
- No round generation exists (`crates/consensus/src` has no such counter). Cross-round stale results
  are discarded structurally — `reset_round` drops `ProposalState` and its futures — but the
  *intra-round* check that would catch issue 3's race (a simulation whose parent moved under it
  returns a `BundleGasDetails` stamped with a parent that no longer matches) does not exist.
- Nothing records the construction parent with a submission or the inclusion parent when a bundle
  lands, so the mismatch PLAN.md accepts is accepted without the visibility it asks for in exchange.
- The fee-config half of criterion 1 *is* tested
  (`a_config_update_mid_round_does_not_change_the_round_being_built`); the head-change /
  same-height-reorg / stale-result-rejected half is not.

**Status.** `main`'s `BundleGasDetails` has no parent at all, so the absence of a check is inherited;
the dead accessor is this branch's. Cheap partial fix now: in `try_build_proposal`, reject when
`gas_info.parent() != handles.block_height`.

---

## 7. The test harness cannot start — no config deployment, zero address, zero hash `[PR A.8]`

*Owned by tickets 13, 16, 17 — closed by ticket 50.*

**Verified.**
- `crates/types/constants/src/lib.rs:117-118`: `INTERNAL_TESTNET` has
  `protocol_fee_config_address: Address::ZERO, protocol_fee_config_deployed_block: 0`.
- `:155-181` `try_init` sets `PROTOCOL_FEE_CONFIG_DEPLOYED_BLOCK` only if nonzero and never sets
  `PROTOCOL_FEE_CONFIG_ADDRESS` at all.
- `testing-tools/src/controllers/strom/internals.rs:166-175`:
  `PROTOCOL_FEE_CONFIG_ADDRESS.get().copied().unwrap_or_default()` → zero, then
  `load_from_chain(zero, block_number, hash, ..).await?`, where `block_number` is
  `best_block_number()` (`:129`) — the live tip, positive on any fork and after the first devnet block.
- `testing-tools/src/controllers/strom/harness.rs:304-311`: the same zero-address fallback, and it
  also passes `Default::default()` — a zero `B256` — as the pinning hash.
- `load_from_chain` (`protocol_fees.rs:173-185`): `block_number <= 0` is false, address is zero →
  `Err("PROTOCOL_FEE_CONFIG_ADDRESS is unset ...")`. Startup aborts.
- No file under `testing-tools/`, `bin/testnet/`, or `bin/devnet/` deploys `AngstromProtocolFeeConfig`;
  the only references are the three load sites. The comments at `internals.rs:163-165` and
  `harness.rs:301-303` rely on "a block at or before the deployed block resolves without a provider
  call" — but the deployed block is 0 and the tip is not, so the recorded assumption is the one that
  fails.

**CI reachability.** `bin/testnet/tests/{testnet,e2e_orders}.rs` hold five tests, all
`#[serial_test::serial]`, none `#[ignore]`, each calling `AngstromAddressConfig::INTERNAL_TESTNET.try_init()`
(`testnet.rs:22,50`, `e2e_orders.rs:209,227,330`) before spinning nodes up through this harness. CI's
integration job is `cargo nextest run --package testnet`, so this fails CI as soon as CI is otherwise
green — it is currently red for unrelated formatting / `BYTECODE` reasons, which is why it has not
shown.

**Status.** Ticket 17's fail-closed posture is right for production and wrong to inherit unmodified
in a harness that never deploys the thing being read. Deploy the config against the harness's
Angstrom, init the address and deployed block, and pass a real hash in `harness.rs`.

---

## 8. Allocator capacity limit is mislabeled as rounding `[PR A.7]`

*Owned by tickets 29, 30 — closed by ticket 49.*

**PLAN.md, Conservation:** remainders "split into two buckets accounted separately from each other
and from the configured protocol fee: integer-allocation rounding, and budget the allocator did not
place."

**Verified by hand** through `crates/types/src/uni_structure/pool_swap.rs`, using the reviewer's
counterexample — upward single-range swap, token0 out 100, token1 in 102, budget 5,000:
- Blob pass leaves `current_blob = (100, 102)`, `remaining_donation = 5000`.
- `:304-309`, upward (`!direction`): `c_t0 = 100.saturating_sub(5000) = 0` → bumped to `1`. Blob is
  `(1, 102)`. That saturation is the cap: in the upward direction the allocator can refund at most
  `d_t0 − 1` per range, whatever the budget.
- `:318` `remaining_donation = 5000`; distribution pass `:334`:
  `min(5000, 100.saturating_sub(target_t0 ≈ 1)) = 99`. `remaining_donation = 4901`.
- `:364-368`: `filled_price` is `Some`, so residual is `{ rounding: 4901, unplaced: 0 }`.

4,901 of a 5,000 budget labelled "what integer division left behind" is wrong; it is the allocator's
capacity limit. The downward direction is different — `:305` adds the whole remainder to the blob, so
its leftover genuinely is `inverse_quantity` rounding — which is why the counterexample is upward.
Reachable on every upward swap whose budget exceeds `d_t0 − 1`.

**Status.** Ticket 29 distinguishes "allocator never ran" (`unplaced`) from "ran but could not place
the last units" (`rounding`); a capacity cap is a third thing its design has no name for. Same family as the `(None, None)` mis-attribution ticket 40 closed. No change to the accepted allocation policy is needed — only the attribution.

---

## 9. The two-pool fixture books the wrong input token, and no two-pool bundle is ever settled `[PR A.9]`

*Owned by tickets 32, 33 — closed by ticket 51.*

**PLAN.md, coverage that must not be dropped:** "**two pools sharing token0**, asserting per-pool
application and checked accumulation."

**Verified.**
- `crates/types/src/traits/bundles.rs:1116-1127` `tob_with_gross` → `tob_order` (`:1095-1112`), which
  is `.asset_in(T1).asset_out(T0)` unconditionally.
- `:1324-1414` `two_pools_sharing_token0`: pool A is `(T0, T1)`, pool B is `(T0, T1_B)` (`:1399`), and
  both searchers come from `tob_with_gross`. Pool B's ToB order therefore says `asset_in = T1` while
  the pool's token1 is `T1_B`, and `process_solution` books `external_swap(.., tob.asset_in, ..)`
  against the wrong token.
- The test asserts `solved.rewarded(0)`, `solved.rewarded(1)` and `solved.save(T0)` only. The `T0`
  axis is correct and discriminating (it was mutation-checked); the `T1`/`T1_B` axis is wrong and
  unobserved.
- `crates/types/tests/anvil_settlement.rs:440`: "A pool per scenario" — each scenario settles its own
  single-pool bundle, so no two-pool bundle is ever executed against the contract.

**Status.** The per-pool-vs-aggregate split the test proves is real; PLAN.md's requirement is only
half established. Parameterise the searcher's assets and settle a genuine two-pool bundle on Anvil,
asserting every token delta and `save`.

---

## 10. The acceptance-criterion-4 test never runs `[PR A.10]`

*Owned by ticket 32 — closed by ticket 38.*

`crates/types/tests/anvil_settlement.rs:2` is `#![cfg(feature = "anvil")]`. Nothing enables that
feature: `just test` is `--lib` only, `just test-integration` is `--tests` without `--all-features`,
CI's unit job is `--workspace --exclude testnet` with no features, CI's integration job is
`-p testnet`, and CI's clippy job uses `--all-features` but only compiles. The default invocation
reports success with zero tests.

PLAN.md calls this the criterion that "Hand-written fixtures do not satisfy", and the test itself is
thorough — two rate scenarios, exact `save`, settlement success as the zero-delta proof, reward
growth, three checked mutations — and the reviewer confirms it passes with the feature on. It just
cannot fail anything today, and this branch deleted the fixture test it replaced
(`crates/types/tests/angstrom.rs` plus `solutionlib`). The workflow already exports `ETH_WS_URL`,
the fork URL the harness falls back from (`anvil_settlement.rs:94`).

---

## 11. Pre-**A** replay equivalence is unverified `[PR A.6]`

*Owned by ticket 34 — closed by ticket 39.*

PLAN.md rollout step 6 requires pre-**A** replay to stay byte-exact. Ticket 34 argues — correctly —
that the baked-in const makes a legacy `f64` branch unnecessary *provided no recorded block reaches
the divergence bound*, and pins the arithmetic with
`the_deployed_split_reproduces_the_legacy_f64_path_below_its_bound`: the two paths agree for every
`total_user_fees` below `3_002_399_751_580_333` and diverge by exactly one unit at it. That test
passes; the reviewer cites the same test as demonstrating the divergence.

The empirical half was never run — no recorded blocks were replayed and diffed — because the replay
inputs (S3 credentials, recorded block ids, archive fork URL) were unavailable. What is open is one
magnitude question: did any pre-**A** per-pool `total_user_fees` reach ~3.0e15 t0 units (0.003 of an
18-decimal token in fees from one batch)?

**Where the reviewer and the branch differ is disposition, not fact.** The reviewer reads the
handoff as requiring the legacy branch unconditionally; ticket 34 makes it conditional on the
measurement. Answer the magnitude question first — it is a lookup — and build the branch only if it
comes back over the bound. If the handoff owner reads it unconditionally, ticket 39's step 3 becomes
unconditional.

---

## 12. Two behaviour changes to existing subsystems, outside PLAN.md's scope

*Owned by tickets 14, 18 — closed by ticket 42.*

PLAN.md's node-changes table assigns `crates/eth/src/manager.rs` one job: "Refresh on canonical
commit and reorg before releasing the block update." Two other existing behaviours moved with it.
Both are defensible; neither is in the plan, and ticket 18 asks for one of them to be called out in
the PR.

- **Telemetry snapshot meaning changed.** `on_canon_update` emitted
  `telemetry_event!(EthUpdaterSnapshot::…)` *before* the commit/reorg handlers on `main`; it now
  emits *after* (`crates/eth/src/manager.rs:151-156`). Required for `protocol_fee_config` to mean
  "in force at this tip", but it also flips `angstrom_tokens`, `pool_store` and `node_set` from pre-
  to post-notification state on every `EthSnapshot`.
- **Periphery log scanning widened.** `apply_periphery_logs` scanned only
  `receipts_by_block_hash(chain.tip_hash())` on `main`; it now walks every block via
  `logs_in_block_order`. Required for the config (ticket 14); as a side effect node/pool logs in
  non-tip blocks are now applied too — a latent `main` bug fixed in passing. The exposure is the
  other direction: `pool_store` and `angstrom_tokens` (`manager.rs:300-302`) have no idempotency, so a
  re-delivered block would double-count where the tip-only scan would not.

---

## 13. The deploy script's Sepolia default is a different Angstrom than the node constants `[PR C.6]`

*Owned by tickets 11, 35, 36 — closed by ticket 53.*

**Verified.** `contracts/script/AngstromProtocolFeeConfig.s.sol:135` returns
`0x9051085355BA7e36177e0a1c4082cb88C270ba90` for Sepolia (copied from `AngstromInspector.s.sol`);
`crates/types/constants/src/lib.rs:239` sets Sepolia `ANGSTROM_ADDRESS` to
`0x3B9172ef12bd245A07DA0d43dE29e09036626AFC`. The reviewer's claim that `0x9051…`'s controller
predates `fastOwner()` — so `verify()` and every `setLpDonationSplits` call revert there — needs
Sepolia RPC to confirm and I could not.

**The consequence worth checking first.** Ticket 36 recorded a Sepolia config deployment at
`0xa58f681e8Db5f9624e03fdfAE899128BD7e3918a`, block `11676439`. If it was deployed with this script's
default, it is bound to `0x9051…`, and `load_from_chain`'s `angstrom()` check (`protocol_fees.rs`)
will reject it against the constants' `0x3B91…` at every Sepolia node start. Whether that is so is a
one-call read of `angstrom()` on the deployed contract. Mainnet is unaffected.

---

## 14. Duplicated `strip_volatile` `[PR std]`

*Owned by ticket 02 — closed by ticket 52.*

`crates/types/primitives/build.rs:126` and `crates/uniswap-v4/build.rs:118` — diffed: byte-identical.
Both added by this branch. Two copies of an artifact-normaliser will drift; share it.

---

## Manually verified

Not tracked as issues and not ticketed. These are settled by hand — against live chain state, by
inspection of the diff, or by a governance decision — and the record of each belongs with the
rollout, not in the tree.

### Live deployments
`crates/types/constants/src/lib.rs:223-226,255-258` point both mainnet and Sepolia at
`0xa58f681e8Db5f9624e03fdfAE899128BD7e3918a`, deployed blocks `25948466` and `11676439`. The same
address on both chains is consistent with one deployer at one nonce — plausible, not suspicious — but
tickets 35 and 36 are the two whose "done when" can only be checked against live state, and that
needs an RPC endpoint not available in this review. The reviewer's mainnet dry run passed `verify()`
at `77263de9`; nothing has been run against Sepolia, and issue 13 gives a specific reason to.

The deploy script has a standalone entry point for exactly this:

```
forge script AngstromProtocolFeeConfigScript --sig "verify(address,address)" \
  0xa58f681e8Db5f9624e03fdfAE899128BD7e3918a <angstrom> --rpc-url <url>
```

It checks runtime code, `angstrom()`, `controller()`, the resolved owner and fast owner, the initial
values, and getter/slot-0 agreement — PLAN.md rollout step 2's full list. Run it on both chains and
keep the output.

### Churn in checked-in artifacts and a dead file
- **Nine unrelated ABI artifacts reformatted.** `abis-types/{Angstrom, ControllerV1,
  IPositionDescriptor, MintableMockERC20, MockRewardsManager, PoolGate, PoolManager,
  PositionFetcher, PositionManager}.json` were rewritten in a different forge output format (key
  ordering, `internalType` placement). Normalised and compared: **all nine are semantically
  identical**, so nothing drifted — but it is a whole-file diff on nine files reviewers must take on
  trust, and the next regeneration on a different forge version will produce another. Pin the forge
  version used for `abis-types` regeneration, or revert the eight files unrelated to this work.
- **Dead file.** `contracts/script/_TmpMockCtl.sol` (added in `5e3c3b85`) has no references anywhere
  — `grep -rn TmpMockCtl contracts/` hits only its own definition. A `ControllerV1` stand-in with a
  zero `fastOwner`, presumably for a manual `anvil_setCode` check. Delete it or move it under
  `contracts/test/`.

### Contract: shadowing warning, internal denominator, event without sender, and two governance decisions `[PR C.1–C.5]`
All five verified against `contracts/src/periphery/AngstromProtocolFeeConfig.sol` at head:

- **C.3 — solc warning 2519.** `forge build --force` reports `Warning (2519): This declaration
  shadows an existing declaration --> src/periphery/AngstromProtocolFeeConfig.sol:45:17` — the
  constructor parameter `angstrom` shadows `function angstrom()` at `:102`. Cosmetic; rename.
- **C.4 — `MAX_SHARE_E6` is `internal`** (`:20`). Making it `public` lets governance tooling read the
  denominator instead of hardcoding `1_000_000`. Adds a fourth view, so PLAN.md's "three views" and
  ticket 10's ABI-shape test both move.
- **C.5 — `LpDonationSplitsSet` carries no sender** (`:38-43`). Adding `msg.sender` (indexed) lets
  the change history distinguish timelock from multisig without a trace lookup. Changes the event
  signature; the Rust binding and eth-manager decode regenerate cleanly since they read by field
  name.
- **C.2 — no compare-and-swap.** The setter writes the full pair unconditionally (`:79-80`), so a
  queued timelock execution silently reverts an intervening fast-owner change; PLAN.md pushes the
  guard to governance tooling. The reviewer's suggested shape — pass the expected current pair and
  revert on mismatch — has precedent in `IAngstromAuth.removePool(StoreKey, PoolConfigStore
  expectedStore, uint256)` (`contracts/src/interfaces/IAngstromAuth.sol:29`, the Angstrom call
  `ControllerV1.removePool` makes; the reviewer attributed it to `ControllerV1` itself). Changes the
  ABI and selector list; a decision.
- **C.1 — fast owner can zero both LP shares in one call, no timelock.** `setLpDonationSplits` accepts
  `msg.sender == fastOwner()` (`:70-73`) with bounds `0..=1_000_000` (`:75-77`). Per PLAN.md by design
  ("Either may call"); the reviewer asks that the breadth be confirmed deliberately, and offers
  raise-only or a floor as narrower emergency levers. A governance decision, not a defect.
