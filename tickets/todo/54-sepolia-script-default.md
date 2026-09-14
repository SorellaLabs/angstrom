# 54 — Fix the Sepolia deploy-script default, and find out what the Sepolia deployment is bound to

**Blocks on:** —
**Closes:** ISSUES.md 14 (PR #680 C.6)
**Follows:** 11, 35, 36

## Files
- `contracts/script/AngstromProtocolFeeConfig.s.sol:128-142` — `angstromOnCurrentChain`
- `crates/types/constants/src/lib.rs:236-262` — the Sepolia block, `ANGSTROM_ADDRESS` and
  `PROTOCOL_FEE_CONFIG_ADDRESS`
- `crates/types/primitives/src/contract_payloads/protocol_fees.rs:195-210` — the `angstrom()` check
  that would reject a mis-bound config at startup

## Goal
One Sepolia Angstrom, agreed on by the script, the constants, and the deployed config.

## Do
1. **Read `angstrom()` on the deployed Sepolia config first.**
   `0xa58f681e8Db5f9624e03fdfAE899128BD7e3918a` at block `11676439` (ticket 36). If it returns
   `0x9051085355BA7e36177e0a1c4082cb88C270ba90` — the script's default — the deployment is bound to
   a different Angstrom than the constants' `0x3B9172ef12bd245A07DA0d43dE29e09036626AFC`, and
   `load_from_chain` will reject it on every Sepolia node start with
   "`protocol fee config … is bound to angstrom 0x9051…, expected 0x3B91…`". That is the first thing
   to know, and it is one `eth_call`.
2. Align the script with the constants: change `:135` to `0x3B91…6AFC`, or drop the Sepolia default
   entirely and require `ANGSTROM_ADDRESS` on Sepolia (the script already errors with
   "set ANGSTROM_ADDRESS" for unknown chains; extending that to Sepolia is the safer shape).
3. If step 1 shows a mis-binding, redeploy against `0x3B91…` and update ticket 36's Sepolia
   address and block. Then run the script's `verify(address,address)` against the new deployment.
4. Confirm or refute the reviewer's on-chain claim that `0x9051…`'s controller predates
   `fastOwner()`. If true, note it in the script so nobody points at it again.

## Done when
- `angstromOnCurrentChain()` on Sepolia returns the same address as `ANGSTROM_ADDRESS` in the
  constants, or requires it explicitly.
- `angstrom()` on the Sepolia config the constants point at returns the constants' Angstrom.
- A Sepolia node built from this branch starts.

## Notes
The mismatch is verified at head: `script:135` returns `0x9051…ba90` (copied from
`AngstromInspector.s.sol`), `constants:239` uses `0x3B91…6AFC`. Mainnet is unaffected — the script's
mainnet default and the constants agree on `0x0000000aa232009084Bd71A5797d089AA4Edfad4`.

The reviewer's claim that a dry run against Sepolia "reverts in `verify()` at `fastOwner()`, and on
that deployment `setLpDonationSplits` would revert for *every* caller" needs Sepolia RPC to confirm;
it could not be checked here. It is consistent with the C.7 comment (also from the reviewer) that
the constants' Sepolia Angstrom has a controller whose `fastOwner()` is `address(0)` — the two
Sepolia deployments have different controller generations, which is exactly how this mismatch
would go unnoticed until a node tried to start.

Step 1 is the whole reason this is a ticket and not a one-line fix. The deploy script's `verify`
entry point checks `angstrom()` against what it was told to expect, so if the Sepolia deployment was
made by running the script with its default, `verify` would have passed against `0x9051…` and
recorded success — and ticket 36 would have written down a config bound to the wrong Angstrom in
good faith.
