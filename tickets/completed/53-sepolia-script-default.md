# 53 — Fix the Sepolia deploy-script default, and find out what the Sepolia deployment is bound to

**Blocks on:** —
**Closes:** ISSUES.md 13 (PR #680 C.6)
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

**As built.** One line of code changed plus a comment; nothing was redeployed.

- **Step 1 — the Sepolia deployment is correctly bound.** `angstrom()` on
  `0xa58f681e8Db5f9624e03fdfAE899128BD7e3918a` returns `0x3B9172ef12bd245A07DA0d43dE29e09036626AFC`,
  the constants' Angstrom, not the script default. So the deployment was not made with the script's
  default, `load_from_chain` will accept it, ticket 36's address and block stand, and step 3
  (redeploy) does not apply.
- **Step 2 — took the constant, not the required env var.** `angstromOnCurrentChain()` on Sepolia
  now returns `0x3B91…6AFC`. Requiring `ANGSTROM_ADDRESS` would leave nothing in the tree for the
  constants to agree with, and lets `0x9051…` be passed by hand again; the env override still works
  for everything else. The comment on the branch records why `0x9051…` must not be pointed at.
  `AngstromInspector.s.sol:41` still defaults to `0x9051…` — deliberately untouched: it only reads,
  and the comment names it as the source of the wrong address.
- **Step 4 — the reviewer's claim is confirmed, and reproduced through the script.** `0x9051…`'s
  controller (slot 0) is `0x73922Ee4f10a1D5A68700fF5c4Fbf6B0e5bbA674`; `owner()` there returns
  `0x9b9202606f77DB144C682650343a10E43aC4C64B` and `fastOwner()` reverts (no such selector). A Sepolia
  dry run of `run()` at head, no `ANGSTROM_ADDRESS` set, simulates a deploy against `0x9051…` and
  reverts inside `verify()` at `0x7392…::fastOwner()` with `EvmError: Revert`. Since the config
  resolves `fastOwner()` on every `setLpDonationSplits` call and fails closed on a reverting lookup
  (`test_auth_revertingFastOwnerLookupFailsClosed`), the setter would revert for every caller on a
  config bound to `0x9051…`.
- **`verify(address,address)` against both live deployments** (read-only, no `--broadcast`; the
  ISSUES.md "Live deployments" record):
  - Sepolia (`--rpc-url https://ethereum-sepolia-rpc.publicnode.com`, angstrom `0x3B91…6AFC`):
    runtime code 1274 bytes, code hash
    `0x8258115b5732d3be05820086a1ca77e3e75f4e8d23bdfb15bab5b987a45b5c99`; `angstrom()` =
    `0x3B9172ef12bd245A07DA0d43dE29e09036626AFC`; `controller()` =
    `0x977c67e6CEe5b5De090006E87ADaFc99Ebed2a7A` (= constants' `CONTROLLER_V1_ADDRESS`, agrees with
    Angstrom's slot 0 and is bound back to `0x3B91…`); owner
    `0xe8F537cF6b77E5224a2e78Ebc4C0abeaEb402C62`, fastOwner `address(0)` (C.7's observation; the
    nonzero requirement is mainnet-only); `userLpShareE6` 750000 (protocol 250000), `tobLpShareE6`
    1000000 (protocol 0); slot 0 `0x…000f4240000b71b0`, agrees with `getLpDonationSplits()`.
  - Mainnet (`--rpc-url https://ethereum-rpc.publicnode.com`, angstrom `0x0000000aa232…fad4`):
    runtime code 1274 bytes, code hash
    `0x441cb09fcd1ad0b6cc42129189432aaa003af1eb8daeaf978bf9f9405faf80b0`; `angstrom()` =
    `0x0000000aa232009084Bd71A5797d089AA4Edfad4`; `controller()` =
    `0x1746484EA5e11C75e009252c102C8C33e0315fD4`; owner
    `0x60D41d9708BBEfd29000d1486C6406Ef23526c01`, fastOwner
    `0xD31C82069da3013fdB16B731AD19076Af9b93105` (both nonzero); `userLpShareE6` 750000 (protocol
    250000), `tobLpShareE6` 1000000 (protocol 0); slot 0 `0x…000f4240000b71b0`, agrees with
    `getLpDonationSplits()`.
  - Both: "Script ran successfully." The code hashes differ across chains because the immutable
    Angstrom is part of the runtime code.
- **Post-fix Sepolia dry run of `run()`**, no `ANGSTROM_ADDRESS` set, no `--broadcast`: the default
  resolves to `0x3B91…`, the simulated deploy passes `verify()` with the same values as above, and
  its code hash is `0x8258…5c99` — identical to the live Sepolia config's. The live bytes are what
  this branch's source compiles to against `0x3B91…`. No `FOUNDRY_PROFILE` or python venv needed.
- **Mutation check.** With the Sepolia return put back to `0x9051…` (comment kept), the same dry run
  reverts at `0x7392…::fastOwner()`; restored exactly, `git diff --stat` on the script matches the
  pre-mutation `6 +++++-`.
- **Not run:** "a Sepolia node built from this branch starts" — needs a Sepolia execution node, not
  available here. The on-chain condition that start would check (`angstrom()` == constants'
  `ANGSTROM_ADDRESS`, config code present) is what `verify()` just confirmed. No `contracts/test`
  covers the script (`AngstromProtocolFeeConfig.t.sol` tests the contract), so no `forge test` was
  run for it. ISSUES.md not edited.

Verification: `forge fmt --check script/AngstromProtocolFeeConfig.s.sol` — clean; `forge build` —
clean (exit 0; the pre-existing `unsafe-typecast` lint notes on `decodeSlot0` still print);
`forge script …:AngstromProtocolFeeConfigScript --sig "verify(address,address)" 0xa58f… <angstrom>
--rpc-url <chain>` — passed on Sepolia and mainnet; `forge script …:AngstromProtocolFeeConfigScript
--rpc-url <sepolia>` (dry run) — reverted at head, passes after the fix.
