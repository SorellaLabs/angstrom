# 52 — Share `strip_volatile` between the two build scripts

**Blocks on:** —
**Closes:** ISSUES.md 15 (PR #680 standards note)
**Follows:** 02

## Files
- `crates/types/primitives/build.rs:126-145` — `strip_volatile`
- `crates/uniswap-v4/build.rs:118-137` — the byte-identical copy
- `crates/types/primitives/Cargo.toml`, `crates/uniswap-v4/Cargo.toml` — `[build-dependencies]`

## Goal
One artifact-normaliser, not two that must be kept in step by hand.

## Do
- Move `strip_volatile` (and `workspace_dir`, which both scripts also duplicate) into one place
  both build scripts can reach. A build script cannot depend on a workspace crate's library
  without ordering hazards, so use a small `build-support` crate under `crates/` listed only in
  `[build-dependencies]`, or an `include!`d shared source file — whichever the workspace already
  has a pattern for.
- Delete both local copies.

## Done when
- `grep -rn "fn strip_volatile" crates/` finds exactly one definition.
- Both crates still regenerate their bindings and the checked-in artifacts are byte-identical
  before and after.

## Notes
The reviewer's standards note, verbatim: "the identical `strip_volatile` helpers in
`crates/types/primitives/build.rs:126` and `crates/uniswap-v4/build.rs:118` require synchronized
maintenance; sharing them would remove drift risk." Diffed here — the two function bodies are
byte-identical. Both were added by this branch.

The function exists for a good reason: solc numbers source units by their index in the compilation
job, so `ast`, `id`, `sourceMap` and `immutableReferences` shift whenever the set of compiled files
changes even though the ABI and bytecode are identical, and `sol!` reads none of them. Stripping
them keeps the checked-in artifacts stable. That is precisely why two copies is a hazard: if one is
updated to strip a new volatile field and the other is not, one crate's artifacts start churning
again and it will look like a real change.
