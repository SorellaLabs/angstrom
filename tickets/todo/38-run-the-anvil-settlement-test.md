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

If CI cannot be given a fork URL, the fallback in `anvil_settlement.rs:94` is the public node
`https://ethereum-rpc.publicnode.com`. Prefer the secret; the public node will rate-limit.
