# 55 — Revert unrelated ABI churn, pin the regeneration toolchain, delete the dead mock

**Blocks on:** —
**Closes:** ISSUES.md 16
**Follows:** 02, 11

## Files
- `abis-types/{Angstrom,ControllerV1,IPositionDescriptor,MintableMockERC20,MockRewardsManager,PoolGate,PoolManager,PositionFetcher,PositionManager}.sol/*.json` — nine artifacts rewritten
- `abis-types/AngstromProtocolFeeConfig.sol/AngstromProtocolFeeConfig.json` — the one that is supposed to be new
- `crates/types/primitives/build.rs` — `WANTED_CONTRACTS` and the `forge bind` invocation
- `contracts/foundry.toml` — where a toolchain pin would live
- `contracts/script/_TmpMockCtl.sol` — dead

## Goal
The branch's artifact diff is exactly the one contract it adds, and cannot silently widen again.

## Do
1. **Revert the eight unrelated artifacts** (all but `AngstromProtocolFeeConfig.json`) to their
   `main` bytes: `git checkout 3690f919 -- abis-types/<each>`. They were normalised and compared —
   all nine are **semantically identical** to `main` after sorting keys, so nothing is lost. What
   changed is forge's output format (key ordering, `internalType` placement), not any ABI.
2. **Pin the toolchain that regenerates them.** The regeneration ran under a different forge
   version than the one that produced the checked-in files, which is the whole cause. Record the
   forge version in `contracts/foundry.toml` (or a `foundryup` pin the build script checks) so a
   future regeneration on a different version produces a visible failure, not a nine-file diff.
   The reviewer notes CI runs forge `1.8.1`; match that.
3. **Delete `contracts/script/_TmpMockCtl.sol`.** Added in `5e3c3b85`, zero references anywhere —
   `grep -rn TmpMockCtl contracts/` hits only its own definition. It is a `ControllerV1` stand-in
   with a zero `fastOwner`, presumably for a manual `anvil_setCode` check during the deployment
   dry run. If it is still wanted for that, it belongs under `contracts/test/` with a name that
   does not start with `_Tmp`.
4. Re-run `cargo check --workspace` — the bindings read `abis-types` at build time and must still
   resolve against the reverted files.

## Done when
- `git diff main -- abis-types/` touches exactly one directory:
  `abis-types/AngstromProtocolFeeConfig.sol/`.
- Regenerating bindings on the pinned toolchain is a no-op on the eight reverted files.
- `_TmpMockCtl.sol` is gone or is a real, referenced test fixture.

## Notes
Nothing here is a correctness risk today — the nine files were checked and are ABI-identical to
`main`. The problem is what the diff costs and what it hides: a whole-file rewrite on nine files
that reviewers must take on trust, on a PR whose contract change is the thing reviewers most need
to be able to see in isolation. A future real ABI drift in one of those nine would be
indistinguishable from formatting noise in the same diff.

Ticket 02's job was to add one contract to `WANTED_CONTRACTS` and regenerate; the regeneration did
that and also rewrote everything else because the local forge disagreed with the one that produced
the originals. `strip_volatile` (ticket 52) already removes the fields that shift between
*compiles*; a version pin is what removes the fields that shift between *forge releases*.
