# 38 — Make the anvil settlement test runnable

**Blocks on:** —
**Closes:** ISSUES.md 10
**Follows:** 32

## Files
- `justfile` — new recipe
- `.github/workflows/build.yaml` — new job
- `crates/types/tests/anvil_settlement.rs:2` — `#![cfg(feature = "anvil")]`, the gate
- `crates/types/Cargo.toml:46-48` — the `anvil` feature

## Goal
PLAN.md's fourth acceptance criterion can fail.

## Do
- Add a `test-anvil` recipe to the justfile running the `anvil` feature, e.g.
  `cargo nextest run -p angstrom-types --features anvil --test anvil_settlement`.
- Add a CI job that runs it. The workflow already sets `ETH_WS_URL: ${{ secrets.CI_ETH_WS_URL }}`
  at the top level, which is the fork URL the harness needs.
- Give it its own job rather than adding `--all-features` to the existing unit job: the test forks
  mainnet and spawns anvil, so its runtime and flake profile do not belong beside the unit tests.

## Done when
- A mutation that breaks settlement — inflating `save_amount`, dropping the reserving `allocate`,
  or handing the ToB allocator the gross — fails CI rather than passing unnoticed.
- `just test` and `just test-integration` are unchanged in runtime.

## Notes
Nothing runs this test today. `just test` is `--lib` only, `just test-integration` is `--tests`
without `--all-features`, the CI unit job excludes `testnet` but passes no features, the CI
integration job runs only `-p testnet`, and the clippy job's `--all-features` compiles it without
executing it. So the one test PLAN.md says is required — "Execute builder-produced bundles against
unchanged Angstrom [...] Hand-written fixtures do not satisfy this" — is dead weight.

That matters more than a normal missing-CI gap because ticket 32 **deleted the fixture test it
replaced** (`crates/types/tests/angstrom.rs` and its `solutionlib` module). Coverage of
`process_solution` against real contracts went from `#[ignore]`d-and-stale to
present-but-never-executed.

Ticket 32's own notes record the three mutations it was checked against. Those are the regressions
this job exists to catch; two of them (`save_amount` inflated, the reserving `allocate` dropped)
revert in `_saveAndSettle` and cannot be reached by any unit test in `crates/types`.

The fork requirement is real and not worth designing around: `deploy_angstrom_create3` mines a hook
address through the create3 factory at `SUB_ZERO_FACTORY`, which does not exist on a bare anvil, and
the call to a codeless address succeeds silently — so a non-forked run hands out an Angstrom with no
code and fails later as an opaque revert. `spawn_anvil_forked` exists for this.

If CI cannot be given a fork URL, the fallback in `anvil_settlement.rs` is the public node
`https://ethereum-rpc.publicnode.com`. Prefer the secret; the public node will rate-limit.

**As built.** `just test-anvil` runs
`cargo nextest run -p angstrom-types --features anvil --test anvil_settlement`, and a new
`cargo-test-anvil` job in `build.yaml` runs the same command with `--cargo-profile ci`. The recipe is
not added to `ci`, `check` or `test`: `just test` and `just test-integration` are byte-identical,
and a local `just ci` still never needs a fork URL.

The job is its own job, per the ticket, mirroring the unit job's setup (checkout with submodules,
Foundry — which is where anvil comes from — mold, nextest, a `rust-cache` keyed
`cargo-test-anvil-${{ runner.os }}`) with the same 25-minute limit. It inherits `ETH_WS_URL` from
the workflow's top-level `env`, so the secret is the fork URL and the public node only the fallback.
No `--retries`: the ticket asked for exactly the command, and a first failure against the fork is
more useful surfaced than retried away; add one if the fork proves flaky. There is no
`.config/nextest.toml`, so nextest's defaults apply — the 60 s slow-timeout only labels the test
`SLOW`, it never terminates it — and the job's timeout is the only cap. Measured locally the test
itself is 3.4 s; the compile is the cost (2m59s from a warm workspace).

**Fork-URL edge not changed.** On a PR from a fork the secret is empty and the workflow sets
`ETH_WS_URL` to `""`, which `std::env::var` returns as `Ok("")` — the fallback in
`anvil_settlement.rs` does not fire and `new_forked("")` fails. The existing `-p testnet` job has
the same exposure; both would need the harness to treat an empty variable as unset, which is not
this ticket's file set.

**Verification.** `cargo nextest run -p angstrom-types --features anvil --test anvil_settlement`
with `ETH_WS_URL` unset (public-node fallback): `PASS [3.403s]`, 1 test run: 1 passed, 0 skipped.
The workflow parses (`ruby -ryaml`) with `cargo-test-anvil` a sibling of the five existing jobs.
`just` is not installed on the machine used, so the recipes were not executed; the `test` and
`test-integration` recipes are untouched in the diff. The CI job itself runs on the PR, not here.

**Mutation check**, one of ticket 32's three as the ticket asks: `t0_donation_vec(tob_lp_budget)`
→ `t0_donation_vec(*gross_tob_reward)` in `crates/types/src/traits/bundles.rs:452`. The same
command: `FAIL [3.660s]`, 0 passed, 1 failed — panicked at `anvil_settlement.rs:450`
(`settle(..).unwrap()`) with `ToB placed 1001 + fee 251 + residual 0 != gross 1001` out of
`check_conservation` (`donation.rs:344`), the builder's own conservation check, exactly as ticket 32
recorded for this mutation. Restored by reversing the same replacement; `git diff --stat` matched
the pre-mutation snapshot line for line and `bundles.rs` carries no diff. The other two mutations
(`save_amount` inflated, the reserving `allocate` dropped) were not re-run here: the ticket asks for
one, and each is a further full recompile of `angstrom-types` in a tree other work was landing in
concurrently.

**Review fixes.** `--retries 1` added to the CI job's command, matching the `-p testnet` integration
job that forks the same way; the "exactly the command" reasoning above overstated the ticket's
"e.g.". The fork-PR edge is closed for this test: `anvil_settlement.rs` now treats an empty
`ETH_WS_URL` as unset (`.ok().filter(|url| !url.is_empty())`), so a fork PR falls back to the public
node instead of `new_forked("")`. The `-p testnet` tests (`testnet.rs`, `e2e_orders.rs`) carry the
same `std::env::var` pattern and were left alone — not this ticket's files. The empty-string path
was not exercised on the machine used (`ETH_WS_URL` had to stay unset); the unset path was:
`PASS [4.419s]`, 1 passed, compile 2m00s on the current tree. The stale `:94` references in the
Notes and above were dropped (ticket 51 moved the fallback to `:97`); ISSUES.md 10 still cites
`:94` and was not edited. Second mutation, one of the two the Notes say this job exists for: the
reserving `allocate(AssetBuilderStage::Reward, t0, save_amount)` at `bundles.rs:569` deleted. Same
command: `FAIL [4.240s]`, 0 passed, 1 failed — panicked at `anvil_settlement.rs:407`
(`bundle reverted: unresolved deltas or a rejected order`), the receipt-status assertion that stands
in for `_saveAndSettle`'s `BundlDeltaUnresolved`, the contract-side revert no unit test reaches.
Restored by the reverse replacement; `git diff` of `bundles.rs` and the full `git diff --stat` both
matched their pre-mutation snapshots. The `save_amount`-inflated mutation was not re-run. The
workflow parses (`ruby -ryaml`), the test file passes `rustfmt +nightly --check`, and
`cargo clippy -p angstrom-types --all-targets --features anvil -- -D warnings` (with the two
pre-existing allows) is clean.
