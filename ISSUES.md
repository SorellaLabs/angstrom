# ISSUES — `feat/on-chain-protocol-fee-state` reviewed against PLAN.md

Scope: **only defects introduced by this branch, in the LP protocol fee config work.** Anything
whose behaviour is also present on `main` is out of scope and is not tracked here, even where
PLAN.md asked this branch to change it — those are recorded under "Excluded" at the bottom so the
decision is auditable, not to be worked.

Every numbered issue below was checked against the merge base (`git merge-base main HEAD` =
`3690f919`) by diffing the specific code it names.

## Verified clean

Checked directly, not inferred:

- `cargo check --workspace --all-targets` — clean.
- `forge test --match-contract AngstromProtocolFeeConfig --ffi` — 29 passed, 0 failed.
- `cargo test -p angstrom-types-primitives --lib protocol_fees` — 17 passed, 0 failed.
- `forge inspect AngstromProtocolFeeConfig storageLayout` — `_userLpShareE6` slot 0 offset 0,
  `_tobLpShareE6` slot 0 offset 4, immutable takes no slot. Matches PLAN.md's decoder contract.
- Selectors match PLAN.md exactly: `setLpDonationSplits` `0xb3226f60`, `getLpDonationSplits`
  `0xfa35d883`, `angstrom` `0xff3ddeb8`, `controller` `0xf77c4791`.
- ABI shape: one state-changing function, three views, no fallback, no receive, no payable.
- The contract source matches PLAN.md's specified contract line for line.
- The nine unrelated `abis-types/*.json` artifacts this branch rewrote are **semantically
  identical** to `main` after normalising key order — reformatting only, no ABI drifted.
- Integer split arithmetic, slot-0 decode, the `serde` bounds guard, the config read's
  code/`angstrom()` validation, log application across every block of a notification, reorg
  inversion from the event's `old*` pair, per-pool application of both splits, the conservation
  checks, and `save_amount = user_protocol_fee + tob_protocol_fee` all match PLAN.md and are
  covered by named tests.

Each issue names the ticket that owns it and the new ticket that closes it.

---

## 1. Configuration is maintained from logs, with no storage read after init

*Owned by tickets 07, 13, 14, 15, 16 — closed by ticket 37.*

**PLAN.md, Reading canonical state, item 3:** "On commit and on reorg, **read the new head** and
publish with that block's identity. A reorg that merely removes an update carries no replacement
event, **which is why storage is the source of truth.** Index `LpDonationSplitsSet` separately for
operator-facing change history and telemetry [...] It is a view over what storage already decided,
**never a second source of configuration.**"

The implementation inverts this. Storage is read exactly once, at init
(`bin/angstrom/src/components.rs:315`). From then on the pair is maintained purely by decoding
`LpDonationSplitsSet` logs (`crates/eth/src/manager.rs:230-234, 309-317`), and reorgs are inverted
from the removed event's `oldUserLpShareE6` / `oldTobLpShareE6` pair
(`crates/eth/src/manager.rs:384`).

Tickets 07, 14 and 15 chose this deliberately and argued it well: the setter writes the full pair
and the event records both sides, so a removed change *is* invertible from the log alone — which is
the specific objection PLAN.md raises. The inversion logic is correct and has four named tests
covering non-tip blocks, last-write-wins, removal, and replacement. This is a reasoned deviation,
not an oversight.

What it gives up is self-healing. The node has no path that ever re-reads storage, so once the
derived pair diverges from chain state it stays diverged and nothing notices:

- **Startup gap.** `components.rs:307` takes the tip from `sub`; line 313 opens `eth_data_sub`. A
  block landing between those two lines is in neither the pinned read nor the cleanser's stream.
- **Missed receipts.** `logs_in_block_order` uses `receipts_by_block_hash(...).unwrap_or_default()`
  (`crates/eth/src/manager.rs:406`). A block whose receipts do not resolve is silently skipped.
- **Reorg without the setter in `old`.** If a reorg notification's `old` chain does not carry the
  `LpDonationSplitsSet` being reverted, `reverted_protocol_fee_config` returns `None` and the node
  keeps running on the reorged-out pair.

The subscribe-ordering and receipt-lookup plumbing predates this branch. What is new is that the
fee configuration now depends on it with no recovery path — under PLAN.md's design each of these
self-corrects at the next canonical head.

At the shipping rates `(750_000, 1_000_000)` the blast radius is zero because the config never
changes. It becomes live the moment rollout step 5 uses the setter.

Cheapest reconciliation that keeps the current design: on each canonical head, re-read slot 0
pinned to that hash and treat disagreement with the log-derived pair as an error. Logs stay the
mechanism; storage becomes the check.

---

## 2. The acceptance-criterion-4 test never runs

*Owned by ticket 32 — closed by ticket 38.*

`crates/types/tests/anvil_settlement.rs:2` is `#![cfg(feature = "anvil")]`. Nothing enables that
feature:

- `just test` → `cargo nextest run --workspace --lib` (integration tests excluded entirely).
- `just test-integration` → `cargo nextest run --workspace --tests` (no `--all-features`).
- CI unit job → `cargo nextest run --workspace --exclude testnet` (no `--all-features`).
- CI integration job → `cargo nextest run --package testnet`.
- CI clippy job → `--all-features`, so it compiles but never executes.

PLAN.md calls this the criterion that "Hand-written fixtures do not satisfy", and the test itself is
thorough — two rate scenarios, exact `save`, settlement success as the zero-delta proof, reward
growth, and three checked mutations. It just cannot fail anything today, and this branch deleted the
fixture test it replaced (`crates/types/tests/angstrom.rs` plus `solutionlib`). Add a
`just test-anvil` recipe and a CI job; the workflow already exports `ETH_WS_URL`, which is the fork
URL the harness falls back from (`anvil_settlement.rs:94`).

---

## 3. Rollout step 6's replay equivalence is unverified

*Owned by ticket 34 — closed by ticket 39.*

PLAN.md rollout step 6 requires pre-**A** replay to stay byte-exact. Ticket 34 argues — correctly —
that the baked-in const makes a legacy `f64` branch unnecessary, and pins the arithmetic half with
`the_deployed_split_reproduces_the_legacy_f64_path_below_its_bound`, which shows the two paths agree
for every `total_user_fees` below `3_002_399_751_580_333` and diverge by one unit at it. That test
passes.

The empirical half was not run: no recorded blocks were replayed and diffed against what was
included. The ticket says so plainly. What is left open is a single magnitude question — did any
pre-**A** per-pool `total_user_fees` reach ~3.0e15 t0 units (0.003 of an 18-decimal token in fees
from one batch)? That is answerable from recorded data alone and should be closed before rollout
step 6 is relied on. Ticket 34's first "Done when" bullet remains unsatisfied.

---

## 4. `DonationResidual` mis-attributes the both-vectors-`None` case

*Owned by tickets 29, 31 — closed by ticket 40.*

**PLAN.md, Conservation:** buckets must be "accounted separately from each other and from the
configured protocol fee", each "attributed to documented allocation steps".

`crates/types/src/traits/bundles.rs:443` (ticket 31 step 2) reports the whole book budget as
`unplaced` — the bucket meaning "the protocol kept it via `collect_extra`" — whenever
`solution.ucp.is_zero()`. But when the ToB vector is also `None`, `total_donation` falls back to
`book_budget` (`bundles.rs:497-500`), that amount is allocated at the Reward stage
(`bundles.rs:564`), and it is emitted as `RewardsUpdate::CurrentOnly { amount: total_donation }`
(`bundles.rs:571-579`) — i.e. it goes to LPs, not the protocol.

The conservation equality still passes (`0 placed + book_budget unplaced == book_budget`), so no
bundle is rejected and settlement is unaffected. But the ledger says "protocol retained" for money
that was donated. Today this is benign: `ucp.is_zero()` implies no filled limit orders and no book
surplus, so `book_budget` is zero on that path. It matters for the step-5 reconciliation component,
which PLAN.md names as the residual's intended consumer.

The `(None, Some(tob))` path is correct and is what `book_noop_after_tob_move` exercises, which is
why the tests do not catch this.

---

## 5. `DonationResidual::total()` is unchecked

*Owned by tickets 29, 30 — closed by ticket 41.*

Ticket 30 step 3: "Use checked arithmetic in the sums — a `u128` overflow must fail the check, not
wrap into agreement." `check_conservation` chains `checked_add` correctly, but calls
`residual.total()`, which is `self.rounding + self.unplaced`
(`crates/types/src/uni_structure/donation.rs:90`) — unchecked.

Not reachable as constructed: every exit sets exactly one of the two buckets nonzero. Worth closing
anyway, since the point of the surrounding code is that overflow must not wrap into agreement with
the check.

---

## 6. Two behaviour changes to existing subsystems, outside PLAN.md's scope

*Owned by tickets 14, 18 — closed by ticket 42.*

PLAN.md's node-changes table assigns `crates/eth/src/manager.rs` one job: "Refresh on canonical
commit and reorg before releasing the block update." Two other existing behaviours moved with it.
Both are defensible; neither is in the plan, and ticket 18 asks for one of them to be called out in
the PR.

- **Telemetry snapshot meaning changed.** `on_canon_update` emitted
  `telemetry_event!(EthUpdaterSnapshot::…)` *before* the commit/reorg handlers on `main`; it now
  emits *after* (`crates/eth/src/manager.rs:151-156`). That is required for `protocol_fee_config` to
  mean "in force at this tip", but it also flips `angstrom_tokens`, `pool_store` and `node_set` from
  pre- to post-notification state on every `EthSnapshot`. Anything consuming that surface
  historically will read differently across this deploy.
- **Periphery log scanning widened.** `apply_periphery_logs` scanned only
  `receipts_by_block_hash(chain.tip_hash())` on `main`; it now walks every block of the notification
  via `logs_in_block_order`. For the config this is required (ticket 14). As a side effect,
  `NodeAdded` / `NodeRemoved` / `PoolConfigured` / `PoolRemoved` in non-tip blocks are now applied
  too — a latent `main` bug fixed in passing. The exposure is the other direction: `pool_store` and
  `angstrom_tokens` have no idempotency, so any notification that re-delivers an already-applied
  block would now double-count where the tip-only scan would not.

---
## Excluded

Recorded here rather than tracked as issues above, in two groups.

### A. PLAN.md requirements whose code is unchanged from `main`

These are **not** branch defects — the behaviour is present on the merge base, so they are `main`
issues and are out of scope per the rule at the top of this file. They are recorded only so the
exclusion is auditable: PLAN.md asks for all six and none was fully implemented, so "was PLAN.md
fully implemented?" is answered *no* on six counts the numbered issues above deliberately do not
track. A.3 is a live correctness bug on `main`, not merely an unmet requirement — worth raising
against `main` separately.

1. **Pool state is re-fetched for final construction.** PLAN.md: "Retain the round's pool snapshots
   too, rather than re-fetching mutable pool state for final construction," and acceptance 1's
   "neither re-read config *or pool state*." `crates/consensus/src/rounds/mod.rs:363` fetches for
   matching; `crates/consensus/src/rounds/proposal.rs:160` fetches again for final construction.
   Both lines are on `main` (`proposal.rs:161` there). The config half of that requirement landed on
   this branch; the pool half was never started.

2. **Submission-time `estimate_gas` is not pinned.** PLAN.md: "Submission-time `estimate_gas` needs
   the same parent state and H+1 environment." `crates/types/src/submission/mod.rs:212` calls
   `estimate_gas(tx)` with no block id. This branch does not touch
   `crates/types/src/submission/mod.rs` at all.

3. **Round reset does not abort the submission task.** PLAN.md: "Round reset must **abort** its
   submission task, not just drop the join handle — a dropped handle leaves the task running.
   Re-check cancellation and identity after async preparation, before signing, and before each
   endpoint send." Also acceptance criterion 2.

   `crates/consensus/src/rounds/proposal.rs:320` stores the submission work as
   `Some(Box::pin(tokio::spawn(submission_future)))`, so `ProposalState.submission_future` holds a
   boxed `JoinHandle`. `reset_round` (`crates/consensus/src/rounds/mod.rs:142`) replaces
   `current_state`, which drops that handle — and dropping a `JoinHandle` detaches the task rather
   than cancelling it. The spawned task goes on to `submit_tx`, signs, and sends to every endpoint
   for a round that has already been invalidated. There is no cancellation token and no identity
   re-check before signing or before each send (`grep -rniE "cancel|abort|generation"
   crates/consensus/src` finds nothing).

   This is the exact hazard PLAN.md names, and it is live — but `proposal.rs:314` on `main` is the
   same `tokio::spawn`, and the field type is identical, so the defect is `main`'s and this branch
   neither introduced nor was required to touch that line. It is listed here because PLAN.md
   explicitly assigns the fix to this work. Both the abort and criterion 2's test belong to it.

4. **`RethDbWrapper`'s selector is shared and mutable.** PLAN.md asks for "an immutable provider per
   parent hash". `main` already has `pub trait SetBlock { fn set_block(&self, block: u64) }` and
   `block: Arc<AtomicU64>` shared by every clone, so the shared-mutable-selector defect is `main`'s.
   This branch *improved* it — the selector became `Arc<RwLock<BlockNumHash>>` so it names a branch
   rather than a height (19), every read routes through one `state()` that errors instead of
   answering from the tip (20), and `simulate_bundle` pins at all, which it did not before (21).
   The residual — a `ValidationRequest::NewBlock` (`crates/validation/src/validator.rs:148`) can
   move the selector under an in-flight simulation — is the `main` defect surviving a partial fix,
   not a new one. Ticket 19 records it.

5. **Nothing checks or records the parent identity.** PLAN.md asks to "discard results that no
   longer match" and to "Record the construction parent and the actual inclusion parent". `main`'s
   `BundleGasDetails` has no parent field at all and no such check or record exists there either, so
   the absence is a `main` condition. This branch added `parent: BlockNumHash` and a `parent()`
   accessor (21); the accessor has no callers and the struct keeps `#[allow(unused)]`. That dead
   accessor is the only branch-new part, and it is simulation-parent plumbing rather than fee-config
   behaviour.

6. **Acceptance criterion 1's head-change half is untested.** "Change the head mid-round — including
   a same-height reorg — and assert the stale result is rejected." The fee-config half of criterion 1
   *is* tested by `a_config_update_mid_round_does_not_change_the_round_being_built`, which asserts
   the round keeps its captured snapshot and that the next round starts from the update. What is
   untested is head-change and stale-result rejection, which is generic round identity — `main` has
   neither the test nor the behaviour.

### B. Branch-introduced, but not PLAN.md functionality

These *are* this branch's, unlike group A. They are housekeeping and verification notes rather than
defects in what PLAN.md specifies, so they are not tracked as issues above.

1. **Churn in checked-in artifacts.**
   - Nine unrelated ABI artifacts reformatted: `abis-types/{Angstrom, ControllerV1,
     IPositionDescriptor, MintableMockERC20, MockRewardsManager, PoolGate, PoolManager,
     PositionFetcher, PositionManager}.json` were rewritten in a different forge output format (key
     ordering, `internalType` placement). Normalised and compared, **all nine are semantically
     identical**, so nothing drifted — but it is a whole-file diff on nine files reviewers must take
     on trust, and the next regeneration on a different forge version will produce another one.
     Worth pinning the forge version used for `abis-types` regeneration, or reverting the eight
     files unrelated to this work.
   - Dead file: `contracts/script/_TmpMockCtl.sol` (added in `5e3c3b85`) has no references anywhere
     — `grep -rn TmpMockCtl contracts/` hits only its own definition. It is a `ControllerV1`
     stand-in with a zero `fastOwner`, presumably for a manual `anvil_setCode` check. Delete it or
     move it under `contracts/test/`.

2. **On-chain deployment not verifiable from this review.**
   `crates/types/constants/src/lib.rs:223-226,255-258` point both mainnet and Sepolia at
   `0xa58f681e8Db5f9624e03fdfAE899128BD7e3918a`, with deployed blocks `25948466` and `11676439`.
   The same address on both chains is consistent with one deployer at one nonce, so this is
   plausible rather than suspicious — but tickets 35 and 36 are the two whose "done when" can only
   be checked against live state, and that needs an RPC endpoint not available here.

   The deploy script has a standalone entry point for exactly this. Before merging, run it against
   both chains and keep the output:

   ```
   forge script AngstromProtocolFeeConfigScript --sig "verify(address,address)" \
     0xa58f681e8Db5f9624e03fdfAE899128BD7e3918a <angstrom> --rpc-url <url>
   ```

   It checks runtime code, `angstrom()`, `controller()`, the resolved owner and fast owner, the
   initial values, and getter/slot-0 agreement — the full list PLAN.md rollout step 2 asks for.
