# ISSUES-2 — PR #680 review comments, verified against head

Source: https://github.com/SorellaLabs/angstrom/pull/680 (`feat/on-chain-protocol-fee-state` → `main`).
Fetched 2026-09-14 via the public REST API (`gh` is unauthenticated here). Local `HEAD` is `209bc2e4`,
which is exactly the PR head the latest review was written against, so every check below is against
the same tree the reviewer saw.

What is on the PR:

| Kind | Count | Author | Date |
| --- | --- | --- | --- |
| Review, `CHANGES_REQUESTED` | 1 | 0xvanbeethoven | 2026-09-14 (on `209bc2e4`) |
| Review, `COMMENTED` | 1 | 0xvanbeethoven | 2026-09-10 (on `77263de9`) |
| Inline review comments | 8 | 0xvanbeethoven | 2026-09-10 |
| Discussion comments | 0 | — | — |

None of the inline comments has a reply. The 2026-09-14 review is the blocking one: ten spec findings
(five P1, five P2) and one standards note. It explicitly excluded this repo's `PLAN.md`, `ISSUES.md`
and `tickets/` and reviewed against "Handoff 57ztqhocj3jv revision 8", so overlap with `ISSUES.md`
below is independent confirmation, not circularity.

**Verdict legend.** CONFIRMED = reproduced or traced in the tree here. CONFIRMED (main) = the claim
is correct *and* the underlying code is byte-identical to `main` — flagged because `ISSUES.md` scopes
those out. UNVERIFIABLE = needs live chain state or the reviewer's private harness. OUTDATED = the
anchored code has since changed.

---

## A. The 2026-09-14 review — spec findings

### A.1 [P1] Simulation changes shared validator state — CONFIRMED (main)

**Claim.** `simulate_bundle` calls `set_block(parent)` on the DB shared with order validation and
other simulations, then clones its cache. `RethDbWrapper` still holds `Arc<RwLock<BlockNumHash>>`
and each cache miss resolves the selector again. A new head changes an in-flight simulation; an old
queued simulation can move live order validation backward.

**Verified.**
- `crates/validation/src/lib.rs:104-124`: one `revm_lru = Arc::new(db)` is handed to
  `SimValidation::new(revm_lru.clone(), ..)` (order validation), `FetchUtils::new(.., revm_lru.clone())`,
  and `BundleValidator::new(revm_lru.clone(), ..)`. All three share the wrapper.
- `crates/validation/src/bundle/mod.rs:125`: `self.db.db.set_block(parent)` then `self.db.clone()`
  then `thread_pool.spawn_raw(..)`.
- `crates/types/src/reth_db_wrapper.rs:59-61`: `state()` reads `self.block.read().hash` on every call,
  so every cache miss re-resolves the live selector.
- `crates/validation/src/validator.rs:148`: `ValidationRequest::NewBlock` calls `set_block` on the
  same wrapper.

Both directions the reviewer names hold: a `NewBlock` moves an in-flight simulation forward, and a
queued historical simulation moves order validation backward. The reviewer's "selector proof, not a
full EVM race reproduction" caveat is honest and I have nothing to add to it.

**Status.** Same finding as `ISSUES.md` Excluded A.4 and ticket 19's declared-unmet clauses. `main`
already has `trait SetBlock { fn set_block(&self, block: u64) }` and `block: Arc<AtomicU64>` shared by
every clone; this branch improved it (hash-addressed, errors instead of tip fallback, pins at all) and
left the sharing. The reviewer's requested correction — an immutable provider per simulation with
caches bound to it — is the right fix and is what ticket 19 said to reopen.

### A.2 [P1] Round reset does not cancel submission — CONFIRMED (main)

**Claim.** Reset replaces `ProposalState`; its submission future holds a spawned Tokio task; dropping
the join handle detaches it. No abort, cancellation token, or round-generation guard. Reset during
nonce lookup, estimation or signing can lead to later endpoint sends.

**Verified.**
- `crates/consensus/src/rounds/proposal.rs:320`:
  `self.submission_future = Some(Box::pin(tokio::spawn(submission_future)))`, field type
  `Option<BoxFuture<'static, Result<bool, JoinError>>>` (`:36`) — a boxed `JoinHandle`.
- `crates/consensus/src/rounds/mod.rs:142` `reset_round` replaces `self.current_state`, dropping it.
  Dropping a `JoinHandle` detaches; it does not abort.
- `crates/types/src/submission/mempool.rs:54-76`: `submit` awaits `build_and_sign_tx_with_gas`
  (nonce, estimate, sign) and then fans `send_raw_transaction` out to every client with no check in
  between. Nothing anywhere in `crates/consensus/src` matches `cancel|abort|generation`.

The reviewer's framing is exactly right: these are sends *not yet made* at reset time, so they fall
outside PLAN.md's accepted "already submitted" limitation.

**Status.** Same as `ISSUES.md` Excluded A.3. `proposal.rs:314` on `main` is the identical
`tokio::spawn` with the identical field type, so this is a `main` defect that PLAN.md assigned to this
work and the branch did not touch. It is a live correctness bug regardless of which branch owns it.

### A.3 [P1] Final construction fetches a second pool state — CONFIRMED (main)

**Claim.** Matching captures pools at `rounds/mod.rs:363`; final construction fetches the mutable
pool map again at `proposal.rs:160`. A canonical update can refresh it after consensus passes
`can_operate` but before it handles the queued reset, so final construction recalculates rewards on
newer pools with old solutions and rates.

**Verified.**
- `crates/consensus/src/rounds/mod.rs:363` and `crates/consensus/src/rounds/proposal.rs:160` are two
  independent `fetch_pool_snapshot()` calls; the second feeds `from_proposal`.
- `crates/consensus/src/manager.rs:330-352`: `poll` drains `canonical_block_stream` (which calls
  `reset_round`) *before* polling `consensus_round_state`, but the pool map is written by the uniswap
  pool manager's own task (`crates/uniswap-v4/src/uniswap/pool_manager.rs:303` on each block, `:317`
  on tick loads). So within one poll, `try_build_proposal` can read post-block pools while the
  pre-block round is still the current state — the reviewer's mechanism is accurate.

**Status.** Same as `ISSUES.md` Excluded A.1. Both `fetch_pool_snapshot()` calls are on `main`
(`proposal.rs:161` there). PLAN.md required carrying the pool snapshots and this branch carried only
the config snapshot. The reviewer's fix — carry the original pool snapshots on the matching result —
is the same shape as what `MatchingOutput` already does for `DonationSplitSnapshot`.

### A.4 [P1] Startup can miss a rate update indefinitely — CONFIRMED, and stronger than ISSUES.md

**Claim.** Pool discovery lets several notifications queue on the original subscription. Startup
consumes one to select parent H, then opens a fresh subscription. A setter in an already-published
H+1 is in neither the H storage snapshot nor the fresh stream, and the cleanser maintains rates only
from logs, so later blocks never repair it. Reviewer reproduced "selected parent 101, missed setter
102, first fresh notification 103" against the real Tokio broadcast channel.

**Verified.** `bin/angstrom/src/components.rs:266-313`:
- `:266` `let mut sub = subscribe_to_canonical_state()`; `:268` one `recv()`.
- `:286-291` `fetch_angstrom_pools(deploy_block, ..)` scans from deployment — the code's own comment
  at `:303-305` says "the fetch pools takes awhile" and admits a gap. Every block that lands during
  the scan queues on `sub`.
- `:307` `sub.recv()` — a `tokio::broadcast::Receiver` returns the *oldest* queued message, so this
  selects the first block after the scan began, not the tip. That is H.
- `:313` `eth_data_sub = subscribe_to_canonical_state()` — a fresh receiver sees only messages sent
  after it is created. Everything queued on `sub` after H is dropped with `sub`.
- `:315` `load_from_chain` at H. The cleanser then only ever applies logs.

This is not a narrow race between two lines, which is how `ISSUES.md` §1 describes it. It is a
deterministic gap of `(blocks elapsed during pool discovery) − 1` on every cold start. The reviewer's
three-block reproduction is exactly what the code does.

**Status.** `ISSUES.md` §1 / ticket 37 cover the remedy (reconcile against storage per head) but
understate the mechanism. Ticket 37 has been updated to name the queue backlog and to add the
reviewer's "keep and reconcile the original subscription" half, which is a `components.rs` change the
reconcile alone does not make. The config mechanism is new on this branch; the subscription plumbing
is `main`'s.

### A.5 [P1] Submission estimation not pinned to the construction parent — CONFIRMED (main)

**Claim.** The submission API takes a target height but no parent hash; the gas closure calls bare
`estimate_gas` (defaults to pending); nonce lookup uses a bare block number; an estimation error
panics through `unwrap`.

**Verified.** `crates/types/src/submission/mod.rs:196-198` `get_transaction_count(from).number(target_block - 1)`;
`:212` `node_provider.estimate_gas(tx).await.unwrap()`. No block id on the estimate, no hash anywhere.

**Status.** Same as `ISSUES.md` Excluded A.2. The reviewer says it plainly: "This code predates the PR
but remains an explicit, unimplemented handoff requirement." `crates/types/src/submission/mod.rs` is
untouched by this branch.

### A.6 [P2] Pre-activation replay does not preserve legacy arithmetic — CONFIRMED; disposition disputed

**Claim.** Historical loading substitutes the initial pair but the builder always uses integer
arithmetic; no activation boundary selects legacy `f64`. At gross `3,002,399,751,580,333` the old LP
amount is one unit larger, and the PR's own boundary test demonstrates it.

**Verified.** All true, and the branch agrees with every factual part:
`the_deployed_split_reproduces_the_legacy_f64_path_below_its_bound` in `protocol_fees.rs` passes and
asserts precisely that divergence (`assert_eq!(legacy_f64_user_split(F64_DIVERGENCE), lp + 1)`).
`load_from_chain` short-circuits to the const at or before the deployed block; `process_solution`
has one arithmetic path.

**Where the reviewer and the branch differ is disposition, not fact.** The reviewer treats the
missing `f64` branch as a defect against the handoff. Ticket 34 treats it as conditional: the branch
is only needed if a recorded pre-**A** block has per-pool `total_user_fees` ≥ the bound (0.003 of an
18-decimal token in fees from one batch), and that magnitude question was never answered because
the replay inputs were unavailable. Both agree no historical divergence has been demonstrated.

**Status.** `ISSUES.md` §3 / ticket 39 — answer the magnitude question first, build the branch only
if it comes back over the bound. If the handoff is read as requiring the branch unconditionally, the
reviewer is right and ticket 39's step 3 becomes unconditional; that is a call for whoever owns the
handoff, not something the tree can settle.

### A.7 [P2] Allocator-limit leftovers are mislabeled as rounding — CONFIRMED, new

**Claim.** Every nonempty allocation assigns its remaining budget to `rounding` and zero to
`unplaced`. Upward single-range swap with token0 out 100, token1 in 102, budget 5,000 yields
donation 99 and remainder 4,901 after the saturation limit — that remainder is capacity retention,
not division rounding.

**Verified by hand** through `crates/types/src/uni_structure/pool_swap.rs`:
- Blob pass leaves `current_blob = (100, 102)`, `remaining_donation = 5000`.
- `:304-309`, upward (`!direction`): `c_t0 = 100.saturating_sub(5000) = 0` → bumped to `1`. Blob is
  `(1, 102)`. The saturation is the cap: in the upward direction the allocator can refund at most
  `d_t0 − 1` per range.
- `:318` `remaining_donation = 5000`; distribution pass, `:334`:
  `min(5000, 100.saturating_sub(target_t0 ≈ 1)) = 99`. `remaining_donation = 4901`.
- `:364-368`: `filled_price` is `Some`, so residual is `{ rounding: 4901, unplaced: 0 }`.

4,901 of a 5,000 budget labelled "what integer division left behind" is wrong. It is the allocator's
capacity limit. The downward direction is different — `:305` adds the whole remainder to the blob, so
its leftover genuinely is `inverse_quantity` rounding — which is why the reviewer specified upward.

**Status.** Not in `ISSUES.md` and not covered by any ticket. It is a third bucket ticket 29's design
has no name for — ticket 29 distinguishes "allocator never ran" (`unplaced`) from "ran but could not
place the last units" (`rounding`), and a capacity cap is neither. Same family as `ISSUES.md` §4
(ticket 40), which is the `(None, None)` mis-attribution; this one is inside `t0_donation_vec` and
reachable on every upward swap whose budget exceeds `d_t0 − 1`. Needs a ticket.

### A.8 [P2] Default testnet startup has no usable config deployment — CONFIRMED, new

**Claim.** The mandatory `load_from_chain` in the harness uses the address from constants;
`INTERNAL_TESTNET` supplies zero, `try_init` never sets that address, and the harness does not deploy
the contract. At a positive init block the loader errors and startup aborts.

**Verified.**
- `crates/types/constants/src/lib.rs:117-118`: `INTERNAL_TESTNET` has
  `protocol_fee_config_address: Address::ZERO, protocol_fee_config_deployed_block: 0`.
- `:155-181` `try_init`: sets `PROTOCOL_FEE_CONFIG_DEPLOYED_BLOCK` only if nonzero; never sets
  `PROTOCOL_FEE_CONFIG_ADDRESS` at all.
- `testing-tools/src/controllers/strom/internals.rs:166-175`:
  `PROTOCOL_FEE_CONFIG_ADDRESS.get().copied().unwrap_or_default()` → zero, then
  `load_from_chain(zero, block_number, hash, ..).await?`. `block_number` is
  `best_block_number()` (`:129`) — the live tip, positive on any fork and after the first devnet block.
- `testing-tools/src/controllers/strom/harness.rs:304-311`: same zero-address fallback, and it also
  passes `Default::default()` — a zero `B256` — as the pinning hash.
- `load_from_chain` (`protocol_fees.rs:173-185`): `block_number <= 0` is false, address is zero →
  `Err("PROTOCOL_FEE_CONFIG_ADDRESS is unset ...")`.
- No file under `testing-tools/`, `bin/testnet/`, or `bin/devnet/` deploys `AngstromProtocolFeeConfig`;
  the only references are the three load sites.
- The code's own comments at `internals.rs:163-165` and `harness.rs:301-303` rely on "a block at or
  before the deployed block resolves without a provider call" — but the deployed block is 0 and the
  tip is not, so the assumption the comment records is the one that fails.

**CI reachability.** `bin/testnet/tests/{testnet,e2e_orders}.rs` hold five tests, all
`#[serial_test::serial]`, none `#[ignore]`, and each calls `AngstromAddressConfig::INTERNAL_TESTNET.try_init()`
(`testnet.rs:22,50`, `e2e_orders.rs:209,227,330`) before spinning up nodes through this harness. CI's
integration job is `cargo nextest run --package testnet`, so this fails CI once CI is otherwise green.
The reviewer notes CI is currently red for unrelated formatting/`BYTECODE` reasons, which is why it
has not shown up.

**Status.** Not in `ISSUES.md` and not covered by any ticket. Ticket 17's "fail closed" posture is
correct for production; the harness needs to deploy the contract against its Angstrom and init the
address (and pass a real hash in `harness.rs`) before starting nodes. Needs a ticket.

### A.9 [P2] Shared-token settlement fixture has inconsistent input assets — CONFIRMED, new

**Claim.** Both searchers in `two_pools_sharing_token0` are built by a helper hardcoding `T1` as
input, but pool B is `T0/T1_B`, so builder accounting credits the wrong input token for B. Assertions
inspect only `T0`, and the Anvil test executes separate single-pool bundles, so passing tests do not
establish shared-token settlement correctness.

**Verified.**
- `crates/types/src/traits/bundles.rs:1116-1127` `tob_with_gross` → `tob_order` (`:1095-1112`), which
  is `.asset_in(T1).asset_out(T0)` unconditionally.
- `:1324-1414` `two_pools_sharing_token0`: pool A is `(T0, T1)`, pool B is `(T0, T1_B)` (`:1399`), both
  searchers come from `tob_with_gross`. So pool B's ToB order says `asset_in = T1` while the pool's
  token1 is `T1_B`; `process_solution` books `external_swap(.., tob.asset_in, ..)` against `T1`.
- The test asserts `solved.rewarded(0)`, `solved.rewarded(1)`, and `solved.save(T0)` only. The `T0`
  side is correct and the per-pool-vs-aggregate split it proves is real; the `T1`/`T1_B` side is
  wrong and unobserved.
- `crates/types/tests/anvil_settlement.rs:440`: "A pool per scenario" — each scenario is its own
  single-pool bundle; no two-pool bundle is ever settled on chain.

**Status.** Not in `ISSUES.md` — I read this test and missed the fixture defect. Ticket 33 owns the
test. The `T0` assertions still discriminate (the test was mutation-checked on that axis), but PLAN.md's
"two pools sharing token0, asserting per-pool application and checked accumulation" is only half
established. Needs a ticket: parameterise the searcher's assets and settle a genuine two-pool bundle
in the anvil harness, asserting every token delta and `save`.

### A.10 [P2] Settlement acceptance test is skipped by default CI — CONFIRMED

**Claim.** `anvil_settlement.rs` is gated on feature `anvil`; CI does not enable it; the default
invocation reports success with zero tests.

**Verified.** `crates/types/tests/anvil_settlement.rs:2` `#![cfg(feature = "anvil")]`; `justfile`
`test`/`test-integration` and both CI test jobs (`.github/workflows/build.yaml`) pass no features; only
the clippy job uses `--all-features`, which compiles without running. Reviewer ran it with the feature
and it passed one test covering both rate scenarios — consistent with ticket 32's notes.

**Status.** Same as `ISSUES.md` §2 / ticket 38.

### A.11 [Standards, non-blocking] Duplicated `strip_volatile` — CONFIRMED

`crates/types/primitives/build.rs:126` and `crates/uniswap-v4/build.rs:118` — diffed the two function
bodies: byte-identical. Both were added by this branch. Not in `ISSUES.md`; low priority, but the
reviewer is right that two copies of an artifact-normaliser will drift. Fold into the ticket for A.8
or A.10, or a one-line follow-up.

### A.12 The review's "Validation and limits" — consistent with what I ran

Reviewer: 17 protocol-fee tests, 45 `angstrom-types` lib tests, 23 `angstrom-eth` lib tests, and the
feature-enabled anvil test all pass; Rust formatting check fails; CI red for pre-existing reasons.
My runs here: the 17 `protocol_fees` tests and 29 contract tests pass; `cargo check --workspace
--all-targets` is clean; the full `angstrom-types --lib` run was killed by memory pressure twice and
I could not complete it, so I am taking the reviewer's 45/45 on trust. The formatting failure is
consistent with the 2026-09-10 review's note that the diffs are in files this PR does not touch.

---

## B. The 2026-09-10 review — contract verdict

"No correctness or security defect in the contract; it is safe to deploy on mainnet."

Every verifiable claim in its **Verified** list matches what I checked independently: storage layout
(slot 0, offsets 0 and 4, immutable in no slot), no value path (one nonpayable state-changing
function, three views, no fallback/receive), bounds inclusive `0..=1_000_000`, atomic pair write,
event on every change, 29/29 contract tests. The mainnet dry-run details (controller `0x1746…fD4`,
owner = TimelockController with 5-day delay, fastOwner = Safe threshold 2, `verify()` passing) are
UNVERIFIABLE here without RPC but are exactly what the deploy script's `verify` asserts, and the
constants file carries the matching controller address (`0x1746484EA5e11C75e009252c102C8C33e0315fD4`).

Its "CI is red, but not because of this PR" note is consistent with the 2026-09-14 review and with
what this branch touches.

---

## C. Inline comments (2026-09-10)

| # | Anchor | Verdict | Status |
| --- | --- | --- | --- |
| C.1 | `AngstromProtocolFeeConfig.sol:66` | CONFIRMED (design question) | Open |
| C.2 | `AngstromProtocolFeeConfig.sol:63` | CONFIRMED (optional) | Open |
| C.3 | `AngstromProtocolFeeConfig.sol:45` | CONFIRMED (nit) | Open |
| C.4 | `AngstromProtocolFeeConfig.sol:20` | CONFIRMED (nit) | Open |
| C.5 | `AngstromProtocolFeeConfig.sol:38` | CONFIRMED (nit) | Open |
| C.6 | `AngstromProtocolFeeConfig.s.sol:135` | CONFIRMED (address mismatch); on-chain part UNVERIFIABLE | Open |
| C.7 | `AngstromProtocolFeeConfig.s.sol` (orig 84) | OUTDATED — already fixed at head | Resolved |
| C.8 | `components.rs` (orig 322) | OUTDATED — already fixed at head | Resolved |

### C.1 Fast owner can set both LP shares to 0% in one call, no timelock — CONFIRMED, design point

`setLpDonationSplits` accepts `msg.sender == fastOwner()` (`:70-73`) and bounds are `0..=1_000_000`
(`:75-77`), so yes: the mainnet 2-of-N Safe can zero both LP shares effective next bundle. This is
per PLAN.md ("Either may call") and PLAN.md's authorization section is explicit that the fast-owner
path bypasses the timelock by design. The reviewer's alternatives (fast owner may only *raise* LP
shares, or a floor) are a governance decision, not a defect. Worth an explicit yes/no from whoever
owns the handoff; nothing in the tree is wrong.

### C.2 Compare-and-swap on the expected pair — CONFIRMED, optional hardening

The queued-timelock-overwrites-fast-owner-change hazard is real and PLAN.md acknowledges it ("Both
rates always move together ... governance tooling must show both values"). The reviewer's precedent
is slightly misattributed: `expectedStore` is on `IAngstromAuth.removePool(StoreKey, PoolConfigStore
expectedStore, uint256)` (`contracts/src/interfaces/IAngstromAuth.sol:29`) — the Angstrom call that
`ControllerV1.removePool` makes — not on `ControllerV1.removePool`'s own signature. The suggestion
stands regardless: `setLpDonationSplits(expectedUser, expectedTob, newUser, newTob)` reverting on
mismatch would make a stale timelock execution fail loudly instead of silently reverting an
intervening change. It changes the ABI and PLAN.md's selector list, so it is a decision, not a fix.

### C.3 solc warning 2519, constructor parameter shadows `angstrom()` — CONFIRMED

`forge build --force`: `Warning (2519): This declaration shadows an existing declaration.
--> src/periphery/AngstromProtocolFeeConfig.sol:45:17` (shadowed: `function angstrom()` at `:102`).
Cosmetic; rename the parameter.

### C.4 `MAX_SHARE_E6` could be `public` — CONFIRMED

`:20` is `uint32 internal constant MAX_SHARE_E6 = 1_000_000;`. Making it `public` adds a fourth view
to the ABI, which PLAN.md's "three views" statement and ticket 10's ABI-shape test would both need
updating for. Trivial either way.

### C.5 Add `msg.sender` (indexed) to `LpDonationSplitsSet` — CONFIRMED

The event (`:38-43`) carries the old and new pair and nothing else. Adding the sender would let the
change history distinguish timelock from multisig without a trace lookup. Same ABI/selector caveat
as C.4; the Rust binding and the eth manager's decode would regenerate cleanly since they read by
field name.

### C.6 Sepolia script default is unusable — CONFIRMED (mismatch); on-chain claim UNVERIFIABLE

`contracts/script/AngstromProtocolFeeConfig.s.sol:135` still returns
`0x9051085355BA7e36177e0a1c4082cb88C270ba90` for Sepolia; `crates/types/constants/src/lib.rs:239` uses
`0x3B9172ef12bd245A07DA0d43dE29e09036626AFC`. The mismatch is real and unaddressed at head. The
reviewer's claim that `0x9051…`'s controller predates `fastOwner()` (so `verify()` and every
`setLpDonationSplits` call revert there) needs Sepolia RPC to confirm and I could not. Note that
ticket 36 recorded the Sepolia config deployment at block `11676439` bound to *some* Angstrom — if it
was deployed via this script with the default, it is bound to `0x9051…` and the node constants point
at a config for a different Angstrom, which `load_from_chain`'s `angstrom()` check would reject at
startup. That is the one consequence of this comment worth checking against live state before
anything else.

### C.7 `require(fastOwner != address(0))` too strict — OUTDATED, fixed at head

At head the require is gated: `script:85-89` wraps it in `if (block.chainid == MAINNET_CHAIN_ID)`, with
a comment explaining testnet controllers may have a zero fast owner. The comment was anchored on
`bac55258` with no current line, which is GitHub's way of saying the anchored code changed. Resolved.

### C.8 Rollout gate: `expect` panics every node with zero constants — OUTDATED, fixed at head

`bin/angstrom/src/components.rs:315-323` is now `.await.map_err(|e| eyre::eyre!(..))?`, not `.expect`,
and mainnet/Sepolia constants are set (`0xa58f681e8Db5f9624e03fdfAE899128BD7e3918a`, blocks
`25948466` / `11676439`). The comment references "ticket 44, 45" — the pre-renumbering ids for what
are now 35 and 36, both in `tickets/completed/`. The ordering requirement it asks to be stated
(deploy → set constants → release) is still worth a line in the PR description. Resolved as a code
finding.

---

## D. What this adds to `ISSUES.md` and `tickets/todo/`

| Review finding | `ISSUES.md` | Ticket | Action |
| --- | --- | --- | --- |
| A.1 shared selector | Excluded A.4 | 19 (declared unmet) | none — `main` |
| A.2 reset does not abort | Excluded A.3 | — | none — `main` |
| A.3 second pool fetch | Excluded A.1 | — | none — `main` |
| A.4 startup queue gap | §1 (understated) | 37 | **ticket 37 updated** with the queue mechanism and the keep-original-subscription half |
| A.5 estimate unpinned | Excluded A.2 | — | none — `main` |
| A.6 legacy arithmetic | §3 | 39 | disposition call for the handoff owner |
| A.7 capacity mislabeled as rounding | **missing** | **none** | **new ticket needed** |
| A.8 harness cannot start | **missing** | **none** | **new ticket needed** — breaks CI's `-p testnet` once CI is green |
| A.9 two-pool fixture wrong on `T1` | **missing** | **none** | **new ticket needed** |
| A.10 anvil test never runs | §2 | 38 | none |
| A.11 `strip_volatile` ×2 | — | — | fold into A.8/A.10 ticket |
| C.1–C.5 contract nits | — | — | governance/ABI decisions; C.3 is a free rename |
| C.6 Sepolia default | — | — | check which Angstrom the Sepolia deployment is bound to |
| C.7, C.8 | — | 35, 36 | already fixed |

Three findings the reviewer caught that the earlier review here did not: A.7, A.8, A.9. All three are
branch-introduced and inside the fee-config work, so they belong in `ISSUES.md`'s numbered list under
the current scoping rule. Say the word and I will add them there and write tickets 43–45.
