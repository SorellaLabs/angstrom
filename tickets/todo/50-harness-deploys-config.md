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
