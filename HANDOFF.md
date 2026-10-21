# HANDOFF — PR #680 regression review (`feat/on-chain-protocol-fee-state`), paused 2026-09-25 ~18:40 EDT

This file is for whoever picks the review up next. Read it top to bottom before running anything. Every
path, command and hazard needed to finish is here. **`ISSUES.md` (repo root, gitignored) holds the findings
themselves**; this file holds the process around them.

---

## 0. TL;DR

- **Goal.** A full regression review of this branch against `main` before it is deployed to mainnet
  production:
  - only serious or breaking regressions **created by this branch**;
  - every issue **proven** by a test that fails on the branch, or a benchmark that is worse;
  - every issue paired with a fix **implemented and verified in a temporary copy**, never in the working tree;
  - everything written up in `ISSUES.md`;
  - no `git add`, `commit` or `push`, ever.
- **Done.**
  - All 5 review areas were covered by parallel reviewers.
  - A read-only multi-lens static sweep ran.
  - An adversarial critique of the proven issues ran.
  - **3 regressions are proven**, each with a failing test and a fix:
    1. **P1.** The node panics and exits when a queued block was reorged out before it is applied. Most
       likely at startup.
    2. **P1.** Every block now logs the entire `CanonStateNotification` at INFO: ~0.8-4.9 MB per block,
       11-70 GB/day.
    3. **P2, observability.** A round reset aborts the bundle-inclusion watch, so the `bundle_included`
       metric and the `block tx result` log are lost.
  - These areas showed **no regression**:
    - bundle economics (22,997-case differential harness vs `main`);
    - consensus/matching/submission apart from #3;
    - validation/order pool/RPC/state provider;
    - contracts/constants/CI/integration.
- **Not done.** The next agent's work, in order:
  1. **Issue 1's fix is incomplete.** Implement the complete fix (three parts: A, B, C) plus two new tests,
     then compile, clippy, test and mutation-check it (section 4, Task 1).
  2. **Independent clean re-run** of the proofs and fixes for issues 1-3 (Task 2).
  3. **Workspace-wide test sweep**, `main` vs branch, compared test by test (Task 3). Both earlier attempts
     were stopped before they finished compiling.
  4. **Clippy with `-D warnings`**, on branch HEAD and on branch plus fixes (Task 4).
  5. **Finalize `ISSUES.md`** (Task 5), then **clean up** (Task 6).

---

## 1. The user's request (verbatim) and standing preferences

> Can you please do a full and complete PR review for this branch? Do not be overly argumentative, but
> ensure that ALL functionality will work as expected relative to the main branch. If there were issues
> already present on the main branch, and are still here on this branch, don't worry about them. Only
> concern yourself with real problems created in this branch, as I will be deploying this branch to live
> production after it's merged in, so EVERY implemented thing in this branch needs to have identical
> functionality to the main branch. Please ensure there is absolutely NO regression. Please extensively
> test and benchmark to prove there are no breaking or serious regressions. I cannot take ANY risks here.
> Please put all your findings in ISSUES.md (only include serious or breaking issues/regressions). Do not
> git add/commit/push anything. Please prove the issues that you find by writing tests for them that fail
> or benches that are suboptimal. Every issue you present to me MUST be proven. ISSUES.md must also contain
> corresponding PROVEN AND VERIFIED (through testing + implementation in tmp directory) fixes for the issues.

Standing preferences, from this session and earlier memory:

- **Git.** Never `git add`, `commit` or `push`. Never modify the working tree except `ISSUES.md` and this
  file. Proof tests and fixes live only in temporary copies.
- **Resources.** Mid-session the user asked that the agents use **over 50% of total CPU** while keeping
  memory **under 50% of RAM** (36 GB machine, so about 18 GB). See section 6 for how that was enforced and
  why CPU could not reach 50%.
- **Pace.** The user is time-sensitive ("Are you almost done? It's been hours."). Prefer finishing the
  listed tasks over opening new investigations.
- **Scope of findings.**
  - No style or naming nits.
  - Nothing already broken on `main`.
  - Behaviour PLAN.md intends is excluded, unless it breaks production.
  - Mark observability-only items as such.
- **Tests.** Run targeted, per-crate tests, not workspace-wide ones, except the planned sweep in Task 3.
- **No new crates or shared helper files** for small helpers (see memory
  `no-new-crates-for-small-helpers`).
- **Lints.** Pre-existing clippy allows on `main`: `-A clippy::result_large_err -A mismatched_lifetime_syntaxes`.
  Formatting uses `rustfmt +nightly-2026-04-23 --edition 2024`.
- **Tools.** `just` and `timeout` are not installed; use cargo directly.

---

## 2. Facts established (do not re-derive)

- **Revisions.** Branch HEAD `684b88af`; merge base with `origin/main` is `3690f919`. The diff is 56
  commits, 118 files, +7946/−1091. The spec is `PLAN.md` (gitignored, in the working tree). Reth is pinned
  at v2.0.0 (`eb4c15e`), with source at
  `~/.cargo/git/checkouts/reth-e231042ee7db3fb7/eb4c15e/`.
- **Production configuration,** verified live on chain.
  - Mainnet `AngstromProtocolFeeConfig` is at `0xa58f681e8Db5f9624e03fdfAE899128BD7e3918a`:
    - code is absent at 25948465 and present at 25948466 (= `PROTOCOL_FEE_CONFIG_DEPLOYED_BLOCK`);
    - `getLpDonationSplits()` returns (750000, 1000000);
    - slot 0 = `0x…0f4240000b71b0`;
    - `angstrom()` = `0x0000000aa232009084Bd71A5797d089AA4Edfad4`;
    - `controller()` = `0x1746484EA5e11C75e009252c102C8C33e0315fD4`.
  - Sepolia holds the same values at the same address, with deploy block 11676439.
  - So production nodes **always** run the post-activation path, with a ToB protocol share of 0.
- **RPC endpoints.** `https://ethereum-rpc.publicnode.com` refuses archive (`--block`) reads;
  `https://eth.drpc.org` serves them.
- **Node binary.** `bin/angstrom/src/lib.rs:57-64` panics on any chain except mainnet and Sepolia, on both
  sides, so the zero-address chains (1301/8453/84532/INTERNAL_TESTNET) cannot be reached.
- **ABI artifacts.** All 9 changed `abis-types` artifacts are identical to `main` in
  `abi`/`methodIdentifiers`/`metadata`/`bytecode`/`deployedBytecode`, so calldata is unchanged.
- **User-fee split.** The f64 → integer change gives identical results below 3,002,399,751,580,333 wei. The
  largest recorded mainnet per-pool fee is 6.83e14.
- **This session runs inside VS Code's integrated terminal**, so every process it starts inherits VS Code's
  Gatekeeper standing (see section 6).

---

## 3. Findings summary

Full text, proofs and fixes are in `ISSUES.md`.

| # | Sev | What | Proof status | Fix status |
|---|-----|------|--------------|------------|
| 1 | P1 | `crates/eth/src/manager.rs`: the per-notification storage reconcile (`state_by_block_hash(tip.hash)`, `:433`) fails for a tip reorged out before it was applied, and `poll` turns the error into `panic!` (`:563-567`) inside the critical `eth handle` task, so the node exits. Most likely at startup, because the backlog is held until `release_canonical_updates()`. Second site: `bin/angstrom/src/components.rs:314-323`, where the init read panics through the `unwrap` in `RethDbProvider::get_code_at`. | Proven: fails on branch (`canonical update not applied: no state found for block 0xbeac…`) against reth's **real** `BlockchainProvider`; passes on `main`. The adversarial critique confirmed faithfulness to reth v2.0.0. | **Partial.** The fix in `C-fix-1` passes the proof and the suites (53/53) and was mutation-checked, but it is **incomplete**: an unused import breaks clippy `-D warnings`; two panic paths remain (a multi-block commit, a double reorg); the init read is not fixed. **Task 1.** |
| 2 | P1 | `crates/eth/src/manager.rs:199`: `tracing::info!(?canonical_updates, …)` Debug-formats the whole notification on every block. `main` logged an 89-byte line. | Proven on real mainnet block 26056533: 793,122 B logged on branch vs 661 B on `main`. | **Complete.** One line: log the tip number and hash. Verified (749 B), mutation-checked. |
| 3 | P2 (obs.) | `crates/consensus/src/rounds/proposal.rs:53-61`: `Drop` aborts the submission task, which also runs the inclusion watch at `:337-369`. The target block's own `NewBlock` resets the round, so `bundle_included` is almost never recorded. | Proven: fails on branch 3/3; passes on `main`; all re-run under the touch rule. | **Complete.** Spawn the watch as its own task. Verified 27/27; ticket 44's no-send guarantee still passes. |

---

## 4. Remaining work: exact steps

### Conventions for every task

```bash
S=/private/tmp/claude-501/-Users-joseph-noorchashm-Desktop-SorellaLabs-GitHub-angstrom/8938eba2-dd42-4be5-9ae8-7a23f60e3428/scratchpad
TOUCH='find crates bin testing-tools -name "*.rs" -o -name "*.toml" -o -name "*.sol" | xargs touch'
```

- **Touch before every cargo command.** In the copy you are about to build, run `eval "$TOUCH"` first,
  every time (see hazard 6.1).
- **Never build two copies in the same target dir at once.** The cargo lock serializes them anyway.
- Use `CARGO_BUILD_JOBS=8`.
- **Run long builds in the background** and wait for completion. Builds of the eth, consensus or validation
  crates take 10-25 minutes on this machine. Never kill a slow build.
- **Check that `$S` still exists.** `/private/tmp` can be purged (reboot, or macOS periodic cleanup). If it is
  gone, rebuild the copies from the preserved evidence in
  `.claude/review-2026-09-25/` (section 5.4):
  1. Make a fresh copy: `rsync -a --exclude target --exclude tickets <repo>/ <copy>/`. For a `main` copy,
     also run `git checkout 3690f919` inside it.
  2. Overlay the saved files.
  3. Rebuild. A target dir starting from nothing costs 30-60+ minutes of dependency compilation.

### Task 1: complete the fix for issue 1 (highest priority)

The full design is in `.claude/review-2026-09-25/issue-1-2-eth/verify-critique.json`, under the
`suggested_fix_changes` of the first `per_issue` entry. Summary:

- **(A) `crates/eth/src/manager.rs`, on top of `C-fix-1`:**
  1. Remove the `BlockHashReader` import; it is unused because the `StateProviderFactory` bound already
     provides `block_hash`.
  2. Add a field `pub(crate) unreconciled_splits: Option<DonationSplits>` to `EthDataCleanser`. Initialize it
     to `None` in `spawn` and in the test constructor (around line 794).
  3. In `apply_periphery_logs`, start with
     `let mut splits = reverted_splits.or(self.unreconciled_splits.take());`.
  4. After the call, if `!self.reconcile_with_storage(..)?`, set `self.unreconciled_splits = splits;` and
     `return Ok(())`. Otherwise publish as before.
  5. Optional: in `reconcile_with_storage`, if `stored != splits` and `!self.storage.is_canonical(tip)?`,
     return `Ok(false)` instead of bailing.
- **(B) `bin/angstrom/src/components.rs:307-323`:** loop over `sub.recv()`.
  - Skip any head whose `node.provider.block_hash(number)? != Some(hash)`.
  - Call `DonationSplitSnapshot::load_from_chain` on the first canonical head.
  - On `Err`, re-check canonicality: if the head is no longer canonical, take the next queued head;
    otherwise return the error.
  - No new import is needed.
- **(C) `crates/types/src/reth_db_provider.rs`:** replace the `.unwrap()` on `provider_at(block_id)` in
  `get_storage_at` and `get_code_at` with the `and_then` / `map_err` form the branch already uses in
  `get_transaction_count`. Without this, the `Err` arm in (B) can never trigger, because the read panics.
- **New tests** in the `crates/eth/src/manager.rs` test module:
  - Give `FakeStorage` per-hash `failing` and `non_canonical` sets. Currently `is_canonical` always returns
    `Ok(true)`.
  - Test (a): `Commit([X1 with setter, X2])` with X2 failing and non-canonical, then
    `Reorg(old=[X2], new=[Y2])`, where Y2 storage holds X1's pair. Expect no panic, and
    `ProtocolFeeConfigUpdated(X1 pair @ Y2)`.
  - Test (b): `Commit(A with setter)` applied, then `Reorg(A→B)` with B failing and non-canonical, then
    `Reorg(B→C)` where C holds the seeded pair. Expect no panic, and the seeded pair published at C.
  - The existing proof `repro_a_queued_block_reorged_out_before_it_is_applied_does_not_panic_the_cleanser`
    must keep passing.
  - The existing helpers `setup_config_eth_manager`, `chain(..)`, `splits_log`, `receipt` and `slot0_word`
    are in the same module.

Procedure:

```bash
cp -cRp $S/C-fix-1 $S/fix-final && cd $S/fix-final
# ... apply (A), (B), (C) and the two tests ...
eval "$TOUCH"
REVIEW_BLOCK_DIR=$S/C-data CARGO_TARGET_DIR=$S/tgt-C CARGO_BUILD_JOBS=8 \
  cargo nextest run -p angstrom-eth -p uniswap-v4 -p angstrom-network -p telemetry -p telemetry-recorder \
  --lib --no-capture --no-fail-fast
#  -> expect every test to pass, including both repro_ proofs and the two new tests (C-fix-1 had 53 passed + 1 skipped)
eval "$TOUCH"
CARGO_TARGET_DIR=$S/tgt-C CARGO_BUILD_JOBS=8 cargo check -p angstrom --all-targets   # compiles components.rs (B)
eval "$TOUCH"
CARGO_TARGET_DIR=$S/tgt-C CARGO_BUILD_JOBS=8 cargo clippy -p angstrom-eth -p angstrom-types -p angstrom \
  --all-targets -- -D warnings -A clippy::result_large_err -A mismatched_lifetime_syntaxes
```

Then:

- **Mutation checks.** Revert each part on its own, re-run, and confirm the matching test fails; restore the
  part afterwards.
  - Revert (A)'s carry-forward: tests (a) and (b) must fail.
  - Revert all of issue 1's fix: the repro must fail.
- **Save the diff** against the original branch, and copy it into `.claude/review-2026-09-25/issue-1-2-eth/`:
  `diff -ru $S/C/crates <dir>/crates > $S/fix-final.diff`.
- **Update `ISSUES.md` issue 1:**
  - replace "Fix status: INCOMPLETE" with the final diff and the verification output;
  - keep the corrected wording (the startup site *panics*; the 1-2% figure is an estimate).
- Rebuild the other copies of `angstrom-eth` from scratch afterwards; `tgt-C` now holds `fix-final`'s
  artifacts (hazard 6.1).

### Task 2: independent clean re-run of issues 1-3

The fresh clones `$S/V-C` (branch plus both issue-1/2 proofs), `$S/V-Cfix` (copy of `C-fix-1`), `$S/V-B`
(branch plus the issue-3 proof) and `$S/V-Bfix` (copy of `B-fix-1`) are prepared, with diffs
`$S/V-*-vs-*.diff`. **No build had completed in them** when work was paused. After Task 1, replace
`V-Cfix` with `fix-final`.

```bash
# issues 1+2: the branch must FAIL both proofs
cd $S/V-C && eval "$TOUCH" && REVIEW_BLOCK_DIR=$S/C-data CARGO_TARGET_DIR=$S/tgt-C CARGO_BUILD_JOBS=8 \
  cargo test -p angstrom-eth --lib repro_ -- --nocapture --test-threads 1
#   expect: panicked ... "canonical update not applied: no state found for block"
#   expect: panicked ... "one block logged 793122 B at INFO"
# the fix must PASS
cd $S/fix-final && eval "$TOUCH" && (same command)            # expect: both ok; on_canon_update logged 749 B
# issue 3
cd $S/V-B && eval "$TOUCH" && CARGO_TARGET_DIR=$S/tgt-B CARGO_BUILD_JOBS=8 cargo nextest run -p consensus --no-fail-fast
#   expect: FAIL review_the_target_block_resetting_the_round_still_records_inclusion
#           ("the inclusion watch was killed before it saw the target block"); repeat 3x with -E 'test(review_the_target_block)'
cd $S/V-Bfix && eval "$TOUCH" && (same command)                 # expect: all pass (27/27), proof passes 3/3
```

- **Proof test locations in the scratchpad copies:**
  - `$S/C/crates/eth/src/manager.rs:2020` (issue 1) and `:2256` (issue 2); the data loader is at `:2132`
    and reads `$REVIEW_BLOCK_DIR`.
  - `$S/B/crates/consensus/src/rounds/proposal.rs:695` (issue 3).
- **`main` controls:**
  - `$S/C-main/crates/eth/src/manager.rs:1013` / `:1183`;
  - `$S/B-main/crates/consensus/src/rounds/proposal.rs:458` (`mod review_tests` at `:383`).
- **Recorded `main` results:** issue 1 PASS; issue 2 PASS, logging 661 B; issue 3 PASS.
- **Target dirs for re-running a `main` control:** use `$S/tgt-C` (touch first) for C-main, or `$S/tgt-Bm`
  for B-main.

### Task 3: workspace-wide test sweep, `main` vs branch

`$S/sweep-main` (pristine `3690f919`) and `$S/sweep-branch` (pristine `684b88af`) exist. The earlier runs
used partially cloned target dirs (`tgt-sweepm`, `tgt-sweepb`) and were stopped while still compiling
**third-party dependencies**; there are no results. **Use `$S/tgt-E` instead.** Fork E built the entire
workspace `--all-targets` there, so its dependency artifacts are complete, and `main`'s `Cargo.lock` differs
from the branch's only by `fslock`. Run the two sweeps **sequentially**, touching sources before each.

```bash
# nextest has no CLI flag for slow-timeout, so give both copies one. main has a test that hangs forever
# (angstrom-types block_sync::test::test_concurrent_reorg_and_block).
cd $S/sweep-main && mkdir -p .config && printf '[profile.default]\nslow-timeout = { period = "60s", terminate-after = 5 }\n' > .config/nextest.toml
cd $S/sweep-branch && { printf '[profile.default]\nslow-timeout = { period = "60s", terminate-after = 5 }\n\n'; cat .config/nextest.toml; } > /tmp/nt && mv /tmp/nt .config/nextest.toml
#   (the branch file's own [[profile.default.overrides]] for testnet still applies)

for L in sweep-main sweep-branch; do
  cd $S/$L && eval "$TOUCH"
  CARGO_TARGET_DIR=$S/tgt-E CARGO_BUILD_JOBS=8 cargo nextest list --workspace --exclude testnet \
    --message-format oneline > $S/$L.list 2> $S/$L.build.log
  CARGO_TARGET_DIR=$S/tgt-E CARGO_BUILD_JOBS=8 cargo nextest run --workspace --exclude testnet \
    --no-fail-fast --retries 0 > $S/$L.run.log 2>&1
done
```

**Compare the two runs.**

- **Test names:** list every test present on `main` but missing on the branch (`comm -23` on the sorted
  lists). Expect none except the deleted, `#[ignore]`d `crates/types/tests/angstrom.rs` test.
- **Results:** list every test that passes on `main` and fails or times out on the branch. Each one must be
  explained, or proven and turned into an issue.
- **Known failures on `main`,** not regressions:

  | Crate | Test(s) on `main` | Why |
  |---|---|---|
  | angstrom-eth | `test_handle_commit`, `test_empty_block_handling` | event-order assumption; the branch fixed these |
  | consensus | 6 × `rounds::tests` | metrics flag never set in `main`'s test setup |
  | validation | `order::state::account::fuzz_tests::proptest_tests::test_tob_priority_invalidation` | proptest |
  | angstrom-types | `block_sync::test::test_concurrent_reorg_and_block` | hangs |

- **Per-area results already recorded:**

  | Crate(s) | Branch | `main` |
  |---|---|---|
  | types lib | 50/50 | 28 + hang |
  | primitives | 47/47 | 30/30 |
  | eth | 32/32 | 13/15 |
  | network | 6/6 | 6/6 |
  | telemetry | 2/2 | 1/1 |
  | uniswap-v4 | 11/11 | 11/11 |
  | consensus + matching | 26/26 | 14 pass, 6 fail |
  | validation + order-pool + rpc | 91/91 | 76/77 |
  | forge | 204/204 | 175/175 |
  | testnet integration | 6/6, one flake passed on retry | not comparable (see below) |

  `main`'s testnet tests discard the runner result, so they cannot fail.

Record the sweep summary in `ISSUES.md` under "Verified so far with no regression found".

### Task 4: clippy with `-D warnings`

Fork E did not finish clippy; `cargo check --workspace --all-targets` showed 0 warnings on the branch. Run
clippy on the branch, and on `main` for comparison, each in its own copy with touch, sequentially in
`tgt-E`:

```bash
cd $S/sweep-branch && eval "$TOUCH" && CARGO_TARGET_DIR=$S/tgt-E CARGO_BUILD_JOBS=8 \
  cargo clippy --workspace --all-targets -- -D warnings -A clippy::result_large_err -A mismatched_lifetime_syntaxes
```

Then do the same in `$S/sweep-main`. Also run it on `fix-final` for the touched crates (Task 1).

**Check first:** whether CI's clippy job passes `-D warnings`. `.github/workflows/build.yaml` (cargo-clippy
job) is the source of truth; `justfile`'s `check-clippy` uses `-D warnings`. Report only new warnings the
branch introduces, and only if they fail CI.

### Task 5: finalize `ISSUES.md`

- Remove the "PRELIMINARY" status line and state the final date and time.
- Issue 1: put in the final complete fix (Task 1) and the clean re-run results (Task 2).
- Issues 2 and 3: add the clean re-run results (Task 2).
- Add the sweep summary (Task 3) and clippy (Task 4) under "Verified so far with no regression found".
- Remove the "Pending" section once it is empty.
- Keep the build-hygiene note and the deployment note (pin the Dockerfile's `foundry:stable` to the version
  CI uses).

### Task 6: clean up and restore the machine

Do these only after confirming with the user.

1. **Restart rust-analyzer** in each VS Code window: Command Palette → "rust-analyzer: Restart server". It
   was stopped with the user's approval to free about 8 GB, and VS Code stopped auto-restarting it after
   five kills.
2. **Delete the scratchpad** once the evidence is final: `rm -rf "$S"`. It holds about 160 GB; free disk went
   from 542 GB to 381 GB during the review. Confirm first that `.claude/review-2026-09-25/` holds everything
   worth keeping. The large fork-A harness inputs and outputs (`A-cases2.json`, 211 MB, and the `A-out*.json`
   files) were deliberately **not** preserved.
3. **Optional.** Ask the user to add Visual Studio Code under System Settings → Privacy & Security →
   Developer Tools, which exempts builds from Gatekeeper scanning (section 6.2).

---

## 5. Where everything is

### 5.1 The real repo, `/Users/joseph-noorchashm/Desktop/SorellaLabs/GitHub/angstrom`

- **`git status` is clean.** The only changes are:
  - `ISSUES.md`: rewritten; gitignored, so not in git history.
  - `HANDOFF.md`: this file, new and untracked.
  - `.claude/review-2026-09-25/`: gitignored evidence bundle, 57 files, 15 MB.
- **Previous `ISSUES.md`.** The 2026-09-16 revision has no copy in git. It is backed up at
  `.claude/review-2026-09-25/ISSUES.2026-09-16.backup.md` and `$S/ISSUES.2026-09-16.backup.md`.
- **Unexplained edit to `rust-toolchain.toml`.** Fork B saw it read `1.94.0 → 1.98.1` at about 15:00. It is
  back to `1.94.0` (mtime 15:11:59) and `git status` is clean.
  - None of this review's agents admitted to changing it.
  - The other Claude session on this machine, `angstrom2-2b`, working in
    `/Users/joseph-noorchashm/Desktop/SorellaLabs/GitHub/angstrom2`, is a plausible source.
  - Worth mentioning to the user.
- **`target/` was used once.** The very first test build (`cargo test -p angstrom-types --lib --no-run` from
  copy `$S/A`) used the real repo's `target/`. The code was identical to HEAD, so the worst effect is a
  one-time rebuild of those crates.

### 5.2 Scratchpad (`$S`) copies

Each copy is a full rsync of the repo (with `.git`, without `target/`). `main` copies were made with
`git checkout 3690f919` inside the copy.

| Copy | What it is | Built with |
|---|---|---|
| `main` | Pristine `main`. Never edited; source for the other `main` copies. | — |
| `A` | Branch + `crates/types/tests/diff_harness.rs` + `tests/solutionlib/` (differential harness) | `tgt-A` |
| `A-main` | `main` + the same harness | `tgt-A` (touched) |
| `A-f64` | Branch with only `split_user` reverted to the f64 formula (isolation run) | `tgt-A` (touched) |
| `A-harness-{main,branch,f64}`, `A-bins/` | Built harness and suite binaries | — |
| `A-cases{,2}.json`, `A-out*.json`, `A-diff.py` | Harness inputs, outputs and diff tool (outputs large; not preserved) | — |
| `B` | Branch + issue 3 proof test; also a `delta_tps` bench patch | `tgt-B` |
| `B-main` | `main` + `mod review_tests` issue 3 control | `tgt-Bm` |
| `B-fix-1` | Branch + proof + issue 3 fix | `tgt-B` |
| `C` | Branch + issue 1/2 proofs (`crates/eth`) + uniswap clone bench (`crates/uniswap-v4/src/uniswap/pool.rs`) | `tgt-C` |
| `C-main` | `main` + the same proofs as controls | `tgt-C` (touched) |
| `C-fix-1` | Branch + proofs + the issue 1 (partial) and issue 2 fixes | `tgt-C` |
| `C-data/` | Mainnet block 26056533: `block.json`, `receipts.json`, `diff.json`, `state.json`, `block.txt`; read via `REVIEW_BLOCK_DIR` | — |
| `D` | Branch + `crates/validation/tests/{state_read_bench,admission_bench}.rs` (+1 dev-dep) | `tgt-D` |
| `D-main` | `main` + the same benches | `tgt-Dmain` |
| `D-bench/` | Bench sources | — |
| `E` | Branch; E's forge, check and testnet runs | `tgt-E` (**full workspace --all-targets**) |
| `E-main` | `main` for forge comparison | — |
| `E-fstable`, `E-f1.8.3`, `foundry-*` | forge-version experiments and foundry binaries | — |
| `sweep-main`, `sweep-branch` | Pristine copies for Task 3 | use `tgt-E` (see Task 3) |
| `V-C`, `V-Cfix`, `V-B`, `V-Bfix` | Fresh clones for Task 2 | `tgt-C`, `tgt-B` |
| `tgt-main` | **Polluted.** Several forks' `main` copies were built here early on. Do not trust without touching. | — |
| `tgt-sweepm`, `tgt-sweepb` | Partial clones, dependencies incomplete. Do not use. | — |

### 5.3 Scripts in `$S`, also copied to `.claude/review-2026-09-25/scripts/`

| Script | What it does | Notes |
|---|---|---|
| `governor.py` | Memory governor. Sums the phys_footprint (top's MEM) of every process whose args contain `8938eba2` and their descendants. Above 48.5% of RAM it SIGSTOPs the youngest `rustc`; below 42% it SIGCONTs. Resumes everything on SIGTERM/SIGINT, logs to `$S/governor.log`. | Run it as a managed background task, **not** `nohup`: `nohup` died with the tool shell. **Currently stopped.** |
| `measure.py` | One-shot: this session's share of instantaneous CPU (two-sample `top`). | |
| `with-anvil-lock.sh <cmd>` | mkdir-mutex (`$S/anvil.lock`) for anything that starts anvil. | This session's harness shares the fixed socket `/tmp/anvil.ipc`. |
| `sweep.sh <copy> <tgt> <label>` | Clone-if-needed + touch + nextest list/run. | Its clone step is **harmful** (hazard 6.3). Its `.done` markers now exist, so it won't clone. |
| `stop-ra.sh` | Kills rust-analyzer until VS Code stops restarting it. | |

### 5.4 Reports and evidence, in `.claude/review-2026-09-25/` (persistent)

- `reports/A.md` … `E.md`: each fork's full report.
  - A, B, C, E are final.
  - D's has placeholders; its numbers are filled in `ISSUES.md` from its logs.
- `reports/static-sweep.json`: the read-only six-lens sweep. It produced 9 candidates:
  - 6 survived, collapsing to issues 1 and 2;
  - 3 were refuted;
  - the critic also cleared 12 more suspects.
- `reports/verify-critique.json` (also in `issue-1-2-eth/`): the adversarial critique of issues 1-3, with
  complete patch text for Task 1.
- `issue-1-2-eth/`:
  - full `manager.rs` for branch-with-proofs, `main`-with-controls, and the fix;
  - `C-fix-1.diff`;
  - `*-proof-tests.vs-*.diff`;
  - the testing-tools `state_provider.rs` fix;
  - `C-data/`.
- `issue-3-consensus/`: full `proposal.rs` for branch, `main` and fix; `B-fix-1.diff`; `B-proof-test.rs`;
  proof diffs.
- `fork-A-harness/`: `diff_harness.rs` for both sides, `solutionlib/`, `A-diff.py`.
- `fork-D-benches/`: `state_read_bench.rs`, `admission_bench.rs`, and the Cargo dev-dep diff.
- `logs/`: suite logs (C, D, E), `C-mut_run.log`, forge suite lists, testnet and anvil logs, `governor.log`.

---

## 6. Hazards and environment gotchas (read all of them)

### 6.1 Cargo reuses artifacts across copies (critical)

**What happens.**
- A workspace crate's artifact slot does not depend on the copy's path. `-C metadata` hashes the
  workspace-relative package id.
- Freshness is judged by mtime (dep-info), not content.
- `cp -cRp` and `rsync -a` preserve mtimes.

So building copy X in a target dir that last built copy Y can **silently link Y's code**.

**Observed in this review.** `C-fix-1` picked up `C-main`'s `angstrom-types-constants` and failed with
`unresolved import crate::primitive::PROTOCOL_FEE_CONFIG_DEPLOYED_BLOCK`. Worse, if the stale code still
compiles, you would test the wrong code without noticing:
- a `main` control could run branch code;
- a branch proof re-run after a fix-copy build would run the **fixed** code and pass.

**Rule.** Run `find crates bin testing-tools -name '*.rs' -o -name '*.toml' -o -name '*.sol' | xargs touch`
in the copy being built, immediately before every cargo command. Alternatively, use one target dir per copy.

**Sanity check.** Confirm the output could only come from the intended copy: branch-only panic messages, or
`strings` on the test binary.

### 6.2 macOS Gatekeeper is the throughput bottleneck

- `syspolicyd` ran at 190-470% CPU, with XprotectService on top, scanning every freshly built executable:
  build scripts, test binaries, proc-macro dylibs. `rustc` processes sat in state "stuck" (in `top`) while
  the machine was ~50% idle.
- **Fix, user action only:** System Settings → Privacy & Security → Developer Tools → enable **Visual Studio
  Code**, the app this session's terminal runs in. It may need a VS Code restart, which ends the session.
- Without that fix, agent CPU share peaked around 25-37% and averaged about 15-20%. The user's "over 50%
  CPU" target was **not** met, for this reason.

### 6.3 Do not clone `target/` directories

- APFS `cp -cRp` of `target/debug` (about 1M files, 164 GB logical) is instant in disk space. But it takes
  6-15 minutes and triggers `fseventsd` (~180%), Spotlight (`corespotlightd`, `mds`) and `syspolicyd`
  storms.
- Reuse existing target dirs with the touch rule instead. `tgt-E` has the most complete dependency set.
- A `.metadata_never_index` file in the scratchpad did not stop Spotlight.

### 6.4 Memory

- 36 GB of RAM with swap nearly full: 11-20 GB swap in use, compressor up to 17 GB.
- At 16 jobs per fork across 5 forks plus 2 sweeps, this review's footprint reached **23.9 GB**, over the
  user's 50% cap. The governor paused `rustc`, and stopping the sweeps brought it back to about 13 GB.
- **8 jobs per build, and at most about 3 concurrent builds, stays under about 17 GB.**
- The governor only pauses; it cannot reduce memory. Control concurrency up front.
- Other big consumers belong to the user and were left alone: Chrome ~5 GB, VS Code, Slack, Codex, the
  Claude app, and three other `claude` CLI sessions.

### 6.5 Another Claude session is running heavy work on this machine

- **`angstrom2-2b`**, in `…/GitHub/angstrom2`, with scratchpad under
  `…-angstrom2/4082e181-…/scratchpad/{baseline,upgrade-diag}`.
- It runs `cargo nextest run --package testnet` and workspace suites, and leaves **orphaned `anvil`
  processes** (ppid 1) on `/tmp/testnet_anvil_*`.
- **Do not kill its processes.** Expect CPU and memory contention and occasional anvil port or IPC
  collisions.

### 6.6 Anvil

- This repo's harness tests share the fixed socket `/tmp/anvil.ipc`. Only one may run at a time: use
  `with-anvil-lock.sh`, and `--test-threads 1` for `-p testnet`.
- The `anvil_settlement` test needs `--features anvil`:
  `cargo nextest run -p angstrom-types --features anvil --test anvil_settlement`.

### 6.7 Other gotchas

- **RPC.** publicnode refuses archive reads; use `https://eth.drpc.org` for `--block` queries.
- **Foundry.** CI pins foundry v1.7.0. The Docker image's `foundry:stable` is 1.5.1 (fine). forge 1.8.3's
  `forge bind` emits no bytecode and trips the branch's new build-script assertion.
- **Primitives build script.** It emits `rerun-if-changed=abis-types/` for a path that doesn't exist, so every
  cargo invocation rebuilds that crate and its dependents. This is already the case on `main`, and it is part
  of why builds are slow.
- **`forge inspect … storageLayout`** adds a `storageLayout` key into `contracts/out/.../Angstrom.json`,
  which the primitives build script copies into tracked `abis-types/`. Never run it in the real repo; if it
  happens, strip the key and `git checkout` the file.
- **zsh does not word-split `$VAR` lists.** Use `bash -c` for `kill $PIDS` and similar.
- **`sleep` in the foreground is blocked** in this harness. Use a background `until`-loop, or Monitor.
- **`sample <pid>` and `du -sh $S` can hang** for minutes on this machine.

---

## 7. How the review was run (for context)

1. **Setup.** The scratchpad copies above, one per area plus `main` copies, each with a target dir cloned
   from the real `target/`.
2. **Five parallel reviewers ("forks"),** one per area:
   - **A.** Bundle construction and fee economics. Result: no regression.
   - **B.** Consensus, matching engine, submission. Result: issue 3.
   - **C.** Block sync, eth manager, startup, networking, telemetry, uniswap pool manager. Result: issues 1
     and 2.
   - **D.** Order validation, bundle validation, order pool, RPC, state provider. Result: no regression.
     Stopped at the pause, with its report nearly final.
   - **E.** Contracts, constants, build, CI, deployment, integration. Result: no regression.
3. **Static regression sweep** (workflow `pr680-static-regression-sweep`). Six read-only lenses:
   crash-surface, liveness-timing, output-equivalence, compatibility, performance, state-and-reorg. Each
   candidate was given to a skeptic to refute, then a completeness critic reviewed the whole.
4. **Verification workflow** (`pr680-verify-confirmed-issues`). Two re-run lanes and a read-only critique.
   Only the critique completed before the pause; it produced the Task 1 findings.
5. **Mid-course corrections.**
   - Resource governor and job limits, at the user's request.
   - rust-analyzer stopped, with the user's approval.
   - The cross-copy artifact hazard was found. Every fork was told to touch sources, and results were re-run
     or checked with `strings`.

All agents, workflows, sweeps and the governor were stopped at the user's request. No process from this
session remains.

---

## 8. Definition of done

- [ ] Issue 1's complete fix (A, B, C, plus 2 tests) is compiled, all tests pass, clippy is clean,
      mutation-checked, and written into `ISSUES.md`.
- [ ] Issues 1-3 are re-run in clean copies: branch fails, fix passes, touch rule followed. Recorded in
      `ISSUES.md`.
- [ ] The workspace sweep, `main` vs branch, is done. No test present on `main` is missing on the branch,
      apart from the known deleted `#[ignore]`d test. No pass→fail that is unexplained or unproven. Recorded.
- [ ] Clippy `-D warnings` is compared on branch vs `main`; only new CI-breaking warnings are reported.
- [ ] `ISSUES.md` status is final: no "PRELIMINARY", no empty "Pending" section.
- [ ] rust-analyzer is restarted; the scratchpad is deleted (with the user's OK); no processes are left
      running.
- [ ] Still true: no `git add`, `commit` or `push` anywhere, and the working tree has no changes besides
      `ISSUES.md` / `HANDOFF.md`.
