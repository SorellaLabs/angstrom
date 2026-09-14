# 50 — Deploy and initialize the config contract in the test harness

**Blocks on:** —
**Closes:** ISSUES.md 7 (PR #680 A.8)
**Follows:** 13, 16, 17

## Overview
Ticket 17 made "no config means no bundle" fail-closed: `load_from_chain` errors on a zero address
past block 0 and startup propagates it. That is right for production. The test harness inherited
it unchanged while never deploying the thing being read: `INTERNAL_TESTNET` carries a zero address
and deployed block 0, `try_init` never sets the address, and neither `internals.rs` nor
`harness.rs` deploys `AngstromProtocolFeeConfig`. Both fall back to `Address::ZERO`, `internals.rs`
reads the live tip (positive on any fork or after the first devnet block), and `harness.rs` also
passes a zero `B256` as the pinning hash. Startup aborts. The comments at both sites assume "a
block at or before the deployed block resolves without a provider call" — but the deployed block
is 0 and the tip is not, so the recorded assumption is the one that fails. CI's five `-p testnet`
tests all go through this path un-ignored; it will fail CI the moment CI is otherwise green.

## Files
- `testing-tools/src/controllers/strom/internals.rs:160-190` — the load site and its `unwrap_or_default()`
- `testing-tools/src/controllers/strom/harness.rs:296-320` — the other load site, zero hash included
- `testing-tools/src/contracts/environment/angstrom.rs` — `AngstromEnv`, where Angstrom is deployed
- `crates/types/constants/src/lib.rs:111-120,155-181` — `INTERNAL_TESTNET`, `try_init`
- `crates/types/tests/anvil_settlement.rs:80-130` — already deploys and inits correctly; the pattern to copy
- `bin/testnet/tests/{testnet,e2e_orders}.rs` — the CI tests that reach this

## Goal
A harness node starts with a real config deployment, read at a real hash.

## Do
1. In `AngstromEnv` (or wherever the harness deploys Angstrom), deploy
   `AngstromProtocolFeeConfig(angstrom, 750_000, 1_000_000)` right after Angstrom, and record its
   address and deployment block.
2. Initialize `PROTOCOL_FEE_CONFIG_ADDRESS` and `PROTOCOL_FEE_CONFIG_DEPLOYED_BLOCK` from that
   deployment — `AngstromAddressBuilder::with_protocol_fee_config` already exists — the way
   `anvil_settlement.rs` does in two stages (chain id first, deployed addresses second).
3. Delete both `unwrap_or_default()` fallbacks. An unset address in the harness is now a harness
   bug and should say so.
4. `harness.rs:304-311` passes `Default::default()` as the block hash. Pass the real tip hash, the
   way `internals.rs` already does with `b.tip().hash()`.
5. Run the five `-p testnet` tests locally and confirm they get past node startup.

## Done when
- `cargo nextest run -p testnet` starts nodes on this branch.
- Neither load site has a zero-address or zero-hash fallback.
- A harness that forgets to deploy the config fails with a message naming it, not with
  "`PROTOCOL_FEE_CONFIG_ADDRESS` is unset".

## Notes
`anvil_settlement.rs` (ticket 32) got this right — it deploys, inits in two stages, and reads at a
real hash — which is why the reviewer could run it and why it is the template. The harness paths
predate it and were never brought in line.

This is not a production concern: mainnet and Sepolia constants are set (ticket 36) and
`components.rs` reads at the real tip hash. It is the harness alone, and it is what stands between
this branch and a green integration job.

Ticket 17's posture is deliberately kept: production still refuses to start without config. The
change here is that the harness provides one, not that the check is loosened.

**As built.** The harness deploys the config it reads; both load sites fail closed by name.

- **Step 1.** `AngstromEnv::new` deploys `AngstromProtocolFeeConfig(angstrom, 750_000, 1_000_000)`
  right after `ControllerV1`, through `deploy_builder(..).send()` + `get_receipt()`, so the address
  and the deployed block both come from the receipt (`execute_then_mine` may or may not mine an
  extra block depending on its 250 ms race, so a block-number read after the fact would be off by
  one either way). `AngstromEnv` records both (`protocol_fee_config()`,
  `protocol_fee_config_deployed_block()`; `new_existing` takes them), `DeployedAddresses` carries
  `protocol_fee_config_address` / `protocol_fee_config_deployed_block`, and `from_globals` reads the
  two `OnceLock`s with the same bare `unwrap` as its four existing fields (its only caller is
  replay's mainnet-fork path, where `try_init_with_chain_id` sets them).
  `crates/types/tests/{bundle,anvil_settlement}.rs` still call `AngstromEnv::new` unchanged; their
  fixtures now carry one more deploy that nothing reads.
- **Step 2.** The `try_init` change (`PROTOCOL_FEE_CONFIG_ADDRESS` set when nonzero) was already in
  the uncommitted working tree when this ticket started — the 4-line hunk in
  `crates/types/constants/src/lib.rs` is not this ticket's edit and was left as is. The init lives
  in `AnvilInitializer::new` right after `AngstromEnv::new`:
  `AngstromAddressBuilder::default().with_protocol_fee_config(..).with_protocol_fee_config_deployed_block(..).build().try_init()`.
  The two stages hold as in `anvil_settlement.rs`: the tests' `INTERNAL_TESTNET.try_init()` runs
  first and skips both (zero address, zero block), then the deployment's values land. Per process:
  nextest runs each test in its own, so each gets its own deployment; under a single-process
  `cargo test` the first test's values would pin the `OnceLock`s for the rest.
- **Step 3.** Both `unwrap_or_default()`s are gone; each site does
  `PROTOCOL_FEE_CONFIG_ADDRESS.get().ok_or_else(..)?` with "the harness did not deploy and
  initialize `AngstromProtocolFeeConfig` (see `AngstromEnv::new` / `AnvilInitializer::new`)". The
  two comments that assumed "at or before the deployed block" are replaced.
- **Step 4.** `harness.rs`: the `block_hash` lookup moved above the load and is the pinning hash.
  `initialize_strom_components_at_block` has no caller in the repo, so this is exercised by
  compilation only.
- **Step 5 — run, with the real outcome.** `cargo nextest run -p testnet --test-threads 1
  --no-fail-fast` (`ETH_WS_URL` unset, so the tests' own `wss://ethereum-rpc.publicnode.com`
  default; one thread because all five share one anvil IPC path and `serial_test` does not reach
  across nextest's per-test processes): 6 tests, 1 passed (`cli::testnet::tests::test_read_config`),
  5 failed — `test_internal_balances_land` 62.3 s, `test_remove_add_pool` 72.7 s,
  `testnet_lands_block` 61.8 s, `testnet_bundle_unlock` 72.7 s, `testnet_deploy` 72.6 s — every one
  with the same panic, `crates/metrics/src/consensus.rs:47` `called Option::unwrap() on a None
  value`: `ConsensusMetricsWrapper::new()` unwraps `METRICS_ENABLED`, which only
  `AngstromTestnetCli::run_all` (`bin/testnet/src/cli/mod.rs:57-60`) sets and none of the five
  tests go through. That code is identical on `main` (`git diff main --
  crates/metrics/src/consensus.rs` is empty; `main`'s `ConsensusManager::new` calls it at
  `manager.rs:95`), so it predates this branch and has nothing to do with the config. The config
  path passed in all five: the captured logs hold zero "did not deploy" / "is unset" / "no code at"
  / "is bound to" lines, and lines that `internals.rs` logs only after the load (`rpc server
  started`, the block-sync registrations, consensus `setting up with validators`) appear in each.
- **Startup, shown directly (temporary, reverted).** With one line added to `testnet_deploy` only —
  `let _ = angstrom_metrics::METRICS_ENABLED.set(false);` — `cargo nextest run -p testnet -E
  'test(=testnet_deploy)'` **passes** in 72.8 s: config deployed at
  `0x3D85e7B30BE9FD7A4bad709D6eD2d130579f9a2E` in block 25977898, three nodes up (three `rpc server
  started`, three consensus setups). The line was removed again (`git diff` on the test file is
  empty). Not kept: the metrics `OnceLock` is a separate harness bug this ticket does not own; a
  one-line `METRICS_ENABLED.set(false)` in the harness or the tests clears it, and until it lands
  the five tests cannot pass on this branch or on `main`.
- **Mutation check.** With the `try_init` in `AnvilInitializer::new` skipped, `testnet_deploy` fails
  in 72.7 s with `spawn_testnet failed: "the harness did not deploy and initialize
  \`AngstromProtocolFeeConfig\` (see \`AngstromEnv::new\` / \`AnvilInitializer::new\`)"` — the
  ticket's message, not "`PROTOCOL_FEE_CONFIG_ADDRESS` is unset". Restored exactly; `git diff
  --stat` and the full `git diff` are byte-identical to the pre-mutation snapshot.
- **Not done.** `bin/testnet/src/devnet.rs:24` keeps `INTERNAL_TESTNET.init()`: it sits in
  `basic_example`, which nothing calls (`run_devnet` goes through `token_prices_update_new_pools`,
  and the real entry `cli/mod.rs:48` already uses `try_init`). If `basic_example` were wired up, its
  unconditional `init()` would pin `PROTOCOL_FEE_CONFIG_ADDRESS` to zero before the harness could
  set it and every node would fail with the new message. The other four tests were not run with
  the experiment line; `testnet_lands_block` is the one that would show a bundle built from the
  deployed splits. ISSUES.md not edited. A stray `anvil --port 8577` (pid 30862, four days old,
  parented to launchd) predates this session and was left alone; it did not collide, and the
  harness's own anvil exited after every test.

Verification: `cargo check -p testing-tools -p testnet --tests` — clean; `cargo check -p replay -p
angstrom-types --features angstrom-types/anvil --tests` — clean (one pre-existing
`mismatched_lifetime_syntaxes` warning in angstrom-types' lib tests, untouched); `cargo clippy -p
testing-tools -p testnet --all-targets -- -D warnings -A clippy::result_large_err -A
mismatched_lifetime_syntaxes` — clean; `cargo +nightly fmt -p testing-tools`; `cargo nextest run -p
testnet --test-threads 1 --no-fail-fast` — 1 passed, 5 failed (all on the pre-existing
`METRICS_ENABLED` panic, none on the config); `testnet_deploy` with the temporary metrics line —
1 passed.

**Review fixes.** Applied after review of the As built above.

- **Done-when 1 (blocking).** `AngstromNodeInternals::new` (`internals.rs`) now opens with
  `let _ = angstrom_metrics::METRICS_ENABLED.set(false);` — the idiom
  `crates/consensus/src/rounds/mod.rs:637` already uses — and `testing-tools` takes on the
  `angstrom-metrics` dependency (one `Cargo.lock` line). It sits on the one path every harness
  node takes (testnet, devnet and replay all build their `ConsensusManager` there); `let _` because
  `bin/testnet`'s `run_all` and `bin/angstrom` decide the flag before they spawn anything, so their
  earlier `set` wins and the harness line is a no-op under the CLI. `crates/metrics` is untouched
  (the bare `unwrap` in `ConsensusMetricsWrapper::new` is out of scope). `cargo nextest run -p
  testnet --test-threads 1 --no-fail-fast` (`ETH_WS_URL` unset): 6 tests, 5 passed —
  `test_read_config`, `test_internal_balances_land` 84.5 s, `test_remove_add_pool` 84.4 s,
  `testnet_lands_block` 84.4 s, `testnet_bundle_unlock` 84.5 s — and `testnet_deploy` failed once
  at 72.7 s with "peer connection failed" (`node.rs:420`, the p2p stage reached after all three
  nodes had logged their config load and consensus setup) while another agent's `cargo check
  --workspace` was saturating the machine; re-run alone on a quiet machine, `testnet_deploy` passes
  in 72.6 s. All five harness tests pass on this branch; the p2p flake under load predates this
  ticket. **Mutation check.** With the `set(false)` line removed, `testnet_deploy` fails at
  `crates/metrics/src/consensus.rs:47` in 72.7 s; restored exactly, `git diff --stat` and the full
  `git diff` byte-identical to the pre-mutation snapshot. The fail-closed mutation check above is
  unaffected — the fix touches neither load site.
- **Step 2, corrected.** The `try_init` hunk in `crates/types/constants/src/lib.rs`
  (`PROTOCOL_FEE_CONFIG_ADDRESS` set when nonzero) is ticket 50's: Do 2 requires it, the Files list
  names `try_init`, and no other ticket claims it. "Not this ticket's edit" above is withdrawn.
- **Notes, corrected.** `anvil_settlement.rs` never deployed `AngstromProtocolFeeConfig` (no
  reference in `HEAD` or the working tree); only its two-stage chain-id / Angstrom-address init was
  the template. The pattern applied here does not change.
- **Not done.** The CI job's missing `--test-threads 1` (all five tests share one anvil IPC path;
  `serial_test` only locks in-process) is pre-existing and outside this ticket.

Verification (review fixes): `cargo clippy -p testing-tools -p testnet --all-targets -- -D
warnings -A clippy::result_large_err -A mismatched_lifetime_syntaxes` — clean; `cargo +nightly fmt
-p testing-tools -- --check` — clean; `cargo nextest run -p testnet --test-threads 1
--no-fail-fast` — 5 passed, 1 failed (`testnet_deploy`, p2p flake under load); `cargo nextest run
-p testnet -E 'test(=testnet_deploy)'` — 1 passed; mutation run of the same — 1 failed at
`consensus.rs:47`.
