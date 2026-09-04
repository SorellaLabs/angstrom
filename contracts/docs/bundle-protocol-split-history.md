# Bundle protocol split: full-history audit

Audit date: 2026-09-04. Repository: `SorellaLabs/angstrom`.

## Conclusion

**No committed change to the effective 75% LP / saved-remainder split was found at any point during the current production Angstrom deployment.** The split was introduced on 2025-05-05, more than two months before production. Every reachable production-era implementation computes the same expression:

```rust
let total_lp_user_donate = (total_user_fees as f64 * 0.75_f64) as u128;
let save_amount = total_user_fees - total_lp_user_donate;
```

The source actually uses the constant `LP_DONATION_SPLIT: f64 = 0.75`. Therefore, “25% protocol” is only shorthand for **the complement saved by the bundle builder**. For exact historical replay, calculate each pool solution as:

```text
F = saturating sum of the filled user orders' t0_fee values in that pool solution
L = (F as f64 * 0.75_f64) as u128
S = F - L
```

Do **not** substitute `F * 25 / 100`, `F / 4`, or decimal arithmetic. The historical code converts `u128` to `f64`, multiplies, then casts back to `u128`; conversion rounding and truncation can produce a different integer for sufficiently large `F`. The aggregation is per `PoolSolution`, not once across an entire multi-pool bundle. See the [production-boundary source](https://github.com/SorellaLabs/angstrom/blob/aedfcbd74296a77fe42e32459f382b86f2d8eea4/crates/types/src/contract_payloads/angstrom/mod.rs#L622-L623) and [current source](https://github.com/SorellaLabs/angstrom/blob/3690f9198321983f3700c5e417827335461e3651/crates/types/src/traits/bundles.rs#L408-L409).

## Production boundary

The production contract is [`0x0000000aa232009084Bd71A5797d089AA4Edfad4`](https://eth.blockscout.com/address/0x0000000aa232009084Bd71A5797d089AA4Edfad4). It was created by [transaction `0x35cd…a12c`](https://eth.blockscout.com/tx/0x35cd06f2d4c1455ea7e1796355632b30a8ac9cf78ba100998c1ef837b078a12c) in block **22,971,782** at **2025-07-22T02:36:23Z**. Archive `eth_getCode` returned no code at block 22,971,781 and 23,569 bytes at 22,971,782. The repository value `ANGSTROM_DEPLOYED_BLOCK = 22971781`, recorded with the production address in [`4b90ee50`](https://github.com/SorellaLabs/angstrom/commit/4b90ee50aaacbcc78f84a7304f3cad2384c550c7), is consequently a one-block-earlier scan sentinel, not the literal creation block.

The latest `origin/main` first-parent commit whose commit timestamp precedes creation was [`aedfcbd7`](https://github.com/SorellaLabs/angstrom/commit/aedfcbd74296a77fe42e32459f382b86f2d8eea4), at 2025-07-22T02:23:21Z. It contains `LP_DONATION_SPLIT = 0.75` and the expression above. A Git commit timestamp does not prove when that commit reached or ran in production. The production-address commit `4b90ee50`, timestamped 2025-07-22T05:09:50Z, contains the same expression. The earliest successful live [`execute(bytes)` transaction](https://eth.blockscout.com/tx/0xa96822403b69c9cace3c28ce662ebc9fb1aef8e59934aaf33bdeb4a5b97182c7) was block 22,972,937 at 2025-07-22T06:28:47Z.

Earlier “mainnet” constants were explicitly temporary: [`96e26576`](https://github.com/SorellaLabs/angstrom/commit/96e265763fb40e973c2c8ef0500e2beeb9b098e8) added the first mainnet data and [`8f4e1218`](https://github.com/SorellaLabs/angstrom/commit/8f4e1218caf935b4e820454b21126f947771d0a9) changed it to “tmp mainnet.” Both also postdate the split's introduction, so using either broader boundary does not change the result.

## Relevant history

| Date | Commit | Finding |
|---|---|---|
| 2025-05-05 | [`6fc9b4ff`](https://github.com/SorellaLabs/angstrom/commit/6fc9b4ff2d20bd0e550c631005f1ea7bf86f3510) | Introduced `LP_DONATION_SPLIT: f64 = 0.75`, donated the computed 75% to LPs, and saved the remainder. Its parent saved all book-user fees; that behavior was pre-production. |
| 2025-06-03 | [`48c51c14`](https://github.com/SorellaLabs/angstrom/commit/48c51c1453e715f68e8defdfdac6becd865f5ef4) | Added a controller comment explicitly describing the bundle fee as “0.25bps saved, 0.75 lps.” |
| 2025-06-03 | [`f8b9c1cc`](https://github.com/SorellaLabs/angstrom/commit/f8b9c1ccf5a8d19b67116b31c18a622178155b09) | Removed that comment only; the builder expression did not change. |
| 2025-08-30 | [`a749afe5`](https://github.com/SorellaLabs/angstrom/commit/a749afe51229b63761e007e7004e8eed4cab6b20) | Changed donation/tick-spacing mechanics, but not the constant or split expression. |
| 2025-11-20 | [`ee3dddc7`](https://github.com/SorellaLabs/angstrom/commit/ee3dddc7ea12093274fcb722fb51a8c878ac9e4c) | Relocated the builder to `crates/types/src/traits/bundles.rs` and the constant to the primitives crate, making it `pub`; value and arithmetic were unchanged. |
| 2026-08-25 | [`3690f919`](https://github.com/SorellaLabs/angstrom/tree/3690f9198321983f3700c5e417827335461e3651) | Audited tip: [`LP_DONATION_SPLIT = 0.75`](https://github.com/SorellaLabs/angstrom/blob/3690f9198321983f3700c5e417827335461e3651/crates/types/primitives/src/contract_payloads/angstrom/mod.rs#L25) and the [same split expression](https://github.com/SorellaLabs/angstrom/blob/3690f9198321983f3700c5e417827335461e3651/crates/types/src/traits/bundles.rs#L408-L409). |

## Audit scope and reproducible checks

The clone was refreshed with `git fetch --all --tags --prune` and was not shallow. It contained 72 branch refs (30 local, 42 remote), no tags, one root commit, and 5,984 commits reachable from those refs.

```sh
git rev-parse --is-shallow-repository                 # false
git rev-list --all --count                            # 5984
git for-each-ref --format='%(refname)'                # 72 refs
git rev-list --all --max-parents=0                    # one root: 554997cb...

git log --all -S'LP_DONATION_SPLIT' -- '**/*.rs'
# ee3dddc7... relocation; 6fc9b4ff... introduction

git log --all -G'LP_DONATION_SPLIT|total_lp_user_donate|save_amount[[:space:]]*=[[:space:]]*total_user_fees|total_user_fees[[:space:]]*-' -- '**/*.rs'
# ee3dddc7... relocation; a749afe5... donation call change; 6fc9b4ff... introduction
```

Additional object/snapshot checks produced:

- **72/72 ref tips**: `0.75` declaration and exact formula; zero missing or other values.
- **109/109 main first-parent snapshots** from `aedfcbd7` through `origin/main`: exact declaration/formula.
- **374/374 full-ancestry snapshots** in the same production-to-main range: exact declaration/formula.
- **225 unique reachable blobs** across the old builder path, relocated builder path, and primitives constant path: 42 declaration-bearing blobs, all `0.75`; 41 formula-bearing blobs, all the exact expression above.
- A broader semantic diff search for user-fee splits, donations, saved amounts, and protocol fees found no alternate bundle split. Later `protocolUnlockedFee` configuration is a distinct on-chain **unlock-fee** mechanism, not this off-chain bundle-fee split.
- As a non-authoritative supplement, `git fsck --unreachable --no-reflogs` found 68 unreachable commits: 57 post-introduction snapshots had the exact 0.75/formula and 11 older snapshots had no split; none had another value. Unreachable objects are not part of repository history reachable from a ref and may disappear after garbage collection.

## What this does not prove

This audit proves what exists in the fetched Git object database; it does not prove which Rust binary every production node executed. The deployed Solidity runtime can be matched to the historical contract build (modulo declared immutables), but the contract authenticates bundle signers, not an off-chain builder commit or image digest. No on-chain field records `LP_DONATION_SPLIT`, its effective block, or the resulting saved amount.

The code also names the complement `save_amount`, not `protocol_fee`. Repository history corroborates a stable **75% LP / 25%-nominal saved** policy, but it does not independently establish that every saved unit is legally or operationally protocol-owned. Before withdrawing funds, the source-history conclusion should be paired with production image/binary provenance and exact bundle replay from block 22,972,937 onward. If that provenance cannot be established, treat the unproven amount as non-withdrawable.
