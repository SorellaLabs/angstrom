# On-chain LP donation splits for user fees and top-of-block auctions

## Objective

Move the hardcoded user-fee `LP_DONATION_SPLIT` into a small on-chain configuration contract, and add a second, independently configurable LP/protocol split for top-of-block (ToB) auction payments in the same contract. Nodes read both rates from canonical state at parent block **H** and use that one snapshot to build the bundle targeting **H+1**.

`AngstromProtocolFeeConfig` is the only contract deployed. Angstrom, `ControllerV1`, and the governance contracts are unchanged and are called only through their existing interfaces. Angstrom does not read this contract or enforce its ratios; it is off-chain configuration that lands in the bundle as ordinary donation and `save` amounts.

## Scope

**In scope**

- The config contract, its setter and getter
- Pinned canonical read of both rates
- One snapshot threaded through both bundle paths
- Integer split arithmetic replacing `f64`
- ToB split applied before the donation merge
- Retained fees through the existing `Asset.save` path
- Fee accounting, reconciliation, and post-settlement verification

**Out of scope**

- On-chain or consensus-level enforcement of the ratios
- Per-pool overrides
- Automated treasury payout scheduling
- Recalling an already-submitted transaction
- Any change to Angstrom, `ControllerV1`, or the governance contracts. `AngstromProtocolFeeConfig` is the only contract deployed, Angstrom never calls it, and the retained ToB portion settles through `Asset.save` exactly as the retained user fee already does.

## Parameters

| Parameter | Base | Initial | Protocol share |
| --- | --- | --- | --- |
| `userLpShareE6` | `total_user_fees` | `750_000` (75%) | `1_000_000 - userLpShareE6` |
| `tobLpShareE6` | gross ToB payment in token0 (`reward_q`) | `1_000_000` (100%) | `1_000_000 - tobLpShareE6` |

Global to this deployment. Bounds inclusive `0..=1_000_000`. Deploying at these values preserves current economics exactly, so activation changes the source of the rate and the arithmetic, not the policy.

## Current behavior

- `LP_DONATION_SPLIT: f64 = 0.75` at [`crates/types/primitives/src/contract_payloads/angstrom/mod.rs:25`](crates/types/primitives/src/contract_payloads/angstrom/mod.rs).
- Applied at [`crates/types/src/traits/bundles.rs:408`](crates/types/src/traits/bundles.rs) in `process_solution`, reached by both `from_proposal` and `for_gas_finalization`.
- `calc_vec_and_reward` in [`crates/types/src/traits/tob.rs`](crates/types/src/traits/tob.rs) returns the gross ToB surplus in token0; `process_solution` passes all of it to `t0_donation_vec`. ToB has no protocol portion today.
- [`ControllerV1.distributeFees`](contracts/src/periphery/ControllerV1.sol) already withdraws saved fees, owner-only.

## Contract

`contracts/src/periphery/AngstromProtocolFeeConfig.sol`:

```solidity
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.26;

import {IAngstromAuth} from "../interfaces/IAngstromAuth.sol";
import {AngstromView} from "./AngstromView.sol";
import {ControllerV1} from "./ControllerV1.sol";

contract AngstromProtocolFeeConfig {
    using AngstromView for IAngstromAuth;

    /// @dev 100% in E6, and the denominator both shares are taken over.
    uint32 internal constant MAX_SHARE_E6 = 1_000_000;

    IAngstromAuth private immutable ANGSTROM;

    // Slot 0: _userLpShareE6 in bytes 0..4, _tobLpShareE6 in bytes 4..8.
    uint32 private _userLpShareE6;
    uint32 private _tobLpShareE6;

    error NotAuthorized();
    error InvalidConfig();

    event LpDonationSplitsSet(
        uint32 oldUserLpShareE6, uint32 newUserLpShareE6,
        uint32 oldTobLpShareE6, uint32 newTobLpShareE6
    );

    constructor(IAngstromAuth angstrom, uint32 initialUserLpShareE6, uint32 initialTobLpShareE6) {
        if (
            address(angstrom) == address(0) || initialUserLpShareE6 > MAX_SHARE_E6
                || initialTobLpShareE6 > MAX_SHARE_E6
        ) {
            revert InvalidConfig();
        }
        ANGSTROM = angstrom;
        _userLpShareE6 = initialUserLpShareE6;
        _tobLpShareE6 = initialTobLpShareE6;
        emit LpDonationSplitsSet(0, initialUserLpShareE6, 0, initialTobLpShareE6);
    }

    function setLpDonationSplits(uint32 newUserLpShareE6, uint32 newTobLpShareE6) external {
        ControllerV1 angstromController = ControllerV1(controller());
        // `owner()` is only reached when the caller is not the fast owner, so a fast-owner call
        // does not depend on the owner lookup. Either lookup reverting fails closed.
        if (
            msg.sender != angstromController.fastOwner() && msg.sender != angstromController.owner()
        ) {
            revert NotAuthorized();
        }
        if (newUserLpShareE6 > MAX_SHARE_E6 || newTobLpShareE6 > MAX_SHARE_E6) {
            revert InvalidConfig();
        }

        (uint32 oldUserLpShareE6, uint32 oldTobLpShareE6) = (_userLpShareE6, _tobLpShareE6);
        (_userLpShareE6, _tobLpShareE6) = (newUserLpShareE6, newTobLpShareE6);

        emit LpDonationSplitsSet(
            oldUserLpShareE6, newUserLpShareE6, oldTobLpShareE6, newTobLpShareE6
        );
    }

    function getLpDonationSplits()
        external
        view
        returns (uint32 userLpShareE6, uint32 tobLpShareE6)
    {
        return (_userLpShareE6, _tobLpShareE6);
    }

    /// @notice The Angstrom deployment this config is bound to. Fixed at construction.
    function angstrom() public view returns (address) {
        return address(ANGSTROM);
    }

    /// @notice The controller whose owner and fast owner may call `setLpDonationSplits`,
    /// resolved from Angstrom's live state on every call.
    function controller() public view returns (address) {
        return ANGSTROM.controller();
    }
}
```

**As built.** The block above reflects the implemented contract with its natspec condensed; `contracts/src/periphery/AngstromProtocolFeeConfig.sol` is authoritative. Private state carries a leading underscore per `ControllerV1`'s convention, which frees the bare names for the getter's named returns so the generated ABI — and the Rust bindings built from it — document themselves.

Deploy with `(existingAngstrom, 750_000, 1_000_000)`.

**Authorization** resolves through `AngstromView.controller()` to the live controller, then its `owner()` or `fastOwner()`, typed as `ControllerV1` directly rather than through a local interface so the two accessors cannot drift from the deployed controller. Either may call; neither the controller itself nor the deployer has standing. If the controller is ever replaced, authority follows the replacement. The mainnet script sets the timelock as owner and the multisig as fast owner, so the multisig can call directly and the timelock through its normal scheduling. Config authority stays separate from withdrawal authority: `distributeFees` remains owner-only.

**Both rates always move together.** The setter writes the full pair, so a queued timelock call will overwrite an intervening fast-owner change. Governance tooling must show both values and re-check the other one before execution.

**Accessors.** `angstrom()` returns the bound deployment; `controller()` returns `ANGSTROM.controller()` and is what the setter itself calls, so authorization and inspection cannot disagree. Both are views: the contract still has exactly one state-changing function, no fallback, no receive, and no path that moves value. `controller()` reads live state rather than a stored copy, so a controller replacement moves configuration authority with it and is observable before the fact.

**Two read paths, one requirement:** a single read must return both rates from one pinned parent state. Decode slot 0, or call `getLpDonationSplits()` — never compose a pair from two reads. Fields stay private so there is no auto-generated single-value getter to compose from.

```text
word = storage[0]
user_lp_share_e6 = word & 0xffff_ffff
tob_lp_share_e6  = (word >> 32) & 0xffff_ffff
```

Verified against compiler output — `forge inspect AngstromProtocolFeeConfig storageLayout` reports `_userLpShareE6` at slot 0 offset 0 and `_tobLpShareE6` at slot 0 offset 4, and the immutable occupies no slot. This layout is part of the interface: changing the declaration order or the widths breaks every off-chain decoder. Selectors for governance tooling: `setLpDonationSplits` `0xb3226f60`, `getLpDonationSplits` `0xfa35d883`, `angstrom` `0xff3ddeb8`, `controller` `0xf77c4791`.

## Node changes

| Location | Change |
| --- | --- |
| [`crates/types/primitives/build.rs`](crates/types/primitives/build.rs) | Add the contract to `WANTED_CONTRACTS`; regenerate bindings. |
| [`crates/types/constants/src/lib.rs`](crates/types/constants/src/lib.rs) | Config address and activation block **A** per network. Rates are not constants. |
| New module under [`crates/types/primitives/src`](crates/types/primitives/src) | `DonationSplits` / `DonationSplitSnapshot` and the split arithmetic. |
| New module under [`crates/eth/src`](crates/eth/src) | Pinned read, code/layout validation, snapshot publication. |
| [`crates/eth/src/manager.rs`](crates/eth/src/manager.rs) | Refresh on canonical commit and reorg before releasing the block update. |
| [`bin/angstrom/src/components.rs`](bin/angstrom/src/components.rs) | Initialize tracking at the node's init block; inject into the block/consensus flow. |
| [`crates/consensus/src/manager.rs`](crates/consensus/src/manager.rs), [`rounds/mod.rs`](crates/consensus/src/rounds/mod.rs), [`rounds/proposal.rs`](crates/consensus/src/rounds/proposal.rs) | One snapshot per round; reuse it for final construction; abort submission work on reset. |
| [`crates/matching-engine/src/lib.rs`](crates/matching-engine/src/lib.rs), [`manager.rs`](crates/matching-engine/src/manager.rs) | Carry the snapshot through `solve_pools` and `MatcherCommand::BuildProposal`. |
| [`crates/validation/src/bundle/validator.rs`](crates/validation/src/bundle/validator.rs) | Carry parent hash on `ValidationRequest::Bundle`; simulate at that state, execute at H+1, return the identity. |
| [`crates/types/src/traits/bundles.rs`](crates/types/src/traits/bundles.rs) | Take `DonationSplits` explicitly; apply both splits; extend `save`. |
| [`crates/types/primitives/.../angstrom/mod.rs`](crates/types/primitives/src/contract_payloads/angstrom/mod.rs) | Delete `LP_DONATION_SPLIT`. |
| Deployment script, [`testing-tools`](testing-tools) | Standalone deploy against the existing Angstrom address; address init for harnesses and replay. |

Fix the callers that break: mocks, benchmarks, testnet setup, replay.

## Snapshot and arithmetic

```rust
pub struct DonationSplits { user_lp_share_e6: u32, tob_lp_share_e6: u32 }

pub struct DonationSplitSnapshot {
    pub block_number: u64,
    pub block_hash:   B256,
    pub splits:       DonationSplits
}

impl DonationSplits {
    pub const DENOM: u32 = 1_000_000;

    /// The only constructor. Rejects either share above DENOM.
    pub fn new(user_lp_share_e6: u32, tob_lp_share_e6: u32) -> eyre::Result<Self>;

    /// Rejects nonzero padding above bit 63.
    pub fn from_slot0(word: U256) -> eyre::Result<Self>;

    pub fn split_user(&self, gross: u128) -> (u128, u128);
    pub fn split_tob(&self, gross: u128) -> (u128, u128);
}

fn split(gross: u128, share_e6: u32) -> (u128, u128) {
    let lp = (U256::from(gross) * U256::from(share_e6)
              / U256::from(DonationSplits::DENOM)).to::<u128>();
    (lp, gross - lp)   // LP rounds down, protocol takes the exact remainder
}
```

`lp + protocol == gross` by construction, and a validated share can never make LP exceed the base. This also drops the old `f64` rounding, so bundles built at 75% may differ by a unit from pre-activation ones — that is part of the activation, and byte-exact replay before **A** must keep the old path.

## Reading canonical state

Read both rates at each canonical head, pinned to that block's hash — locally via the provider, or over RPC with an EIP-1898 block-hash identifier and `requireCanonical: true`. Never `latest` or a bare block number.

1. Subscribe to canonical updates before taking the startup snapshot, then reconcile queued updates.
2. Validate the address holds the expected code for the intended Angstrom immutable. An empty account reads as zero storage and must not be mistaken for two valid 0% settings.
3. On commit and on reorg, read the new head and publish with that block's identity. A reorg that merely removes an update carries no replacement event, which is why storage is the source of truth. Index `LpDonationSplitsSet` separately for operator-facing change history and telemetry: filter by the config address, account for removed blocks, and process every relevant block in a notification. It is a view over what storage already decided, never a second source of configuration.
4. The read must complete before consumers build the corresponding round. Today's cleanser callbacks are synchronous, so the read participates in block synchronization rather than running detached.
5. A failed read skips or retries the round. It never falls back to a stale or default rate. The local provider adapter currently unwraps state-provider errors; those panics must become errors.

## Bundle construction

In `process_solution`, after `calc_vec_and_reward` returns the gross ToB payment and before building the donation vector:

```rust
let (lp_user_fees,  user_protocol_fee) = splits.split_user(total_user_fees);
let (tob_lp_budget, tob_protocol_fee)  = splits.split_tob(gross_tob_reward);

let save_amount = user_protocol_fee
    .checked_add(tob_protocol_fee)
    .ok_or_else(|| eyre::eyre!("retained fees exceed u128"))?;

let tob_donation_vec = tob_vec.t0_donation_vec(tob_lp_budget); // was: full gross
// book donations unchanged, on solution.reward_t0 + lp_user_fees
```

Then the existing merge, `DonationCalculation`, and `RewardsUpdate` / `PoolUpdate` encoding, with `total_donation` computed from the actual merged donations.

Rules that are easy to get wrong:

- Apply each share **once per pool, to that pool's gross total** — not per tick, per fragment, or after assets are aggregated across pools.
- Leave `calc_vec_and_reward`, `calc_reward`, bid ranking, swap quantities, and the post-ToB price alone. The fee divides value the auction already pays; the searcher's obligation and signature are untouched. Ranking stays on gross, so no bid changes hands.
- The ToB share touches only ToB surplus. Not user fees, book surplus, gas, or unlocked-swap fees. `gas_used_asset_0` is not a fee bucket.
- No ToB order means both ToB values are zero. A selected ToB order that fails to evaluate is an error, not zero revenue.
- Running the existing allocator on a smaller budget may shift rewards between tick ranges. Each LP is not promised the configured fraction of its previous claim.

**Conservation.** Have the allocator return its unallocated remainder alongside the vector. Per source, assert `sum(donations) + residual == budget`, then `encoded_tob_donation + tob_protocol_fee + tob_residual == gross_tob_reward`. Residuals are integer-allocation rounding kept through `collect_extra` as today, reported separately from the explicit fee. Reject over-allocation, malformed ranges, and unexplained remainders of any size — no "material" threshold.

**No-ops.** A true no-op (zero deltas, unchanged price and tick) allocates its whole budget to the active range at that source's end state, with zero residual, and fails if that range has no liquidity. A book no-op after a ToB swap uses the post-ToB state. A moving swap with missing range metadata is an error, not a no-op. `Some(empty)` must not silently skip allocation.

**Settlement accounting** reuses the existing retained-fee pattern:

```rust
asset_builder.allocate(AssetBuilderStage::Reward, t0, total_donation);
asset_builder.allocate(AssetBuilderStage::Reward, t0, save_amount);
asset_builder.add_gas_fee(AssetBuilderStage::Reward, t0, save_amount);
```

`add_gas_fee` increments `save` despite its name; the user-fee path already uses it this way. Allocating as well as saving keeps the amount from being counted again by `collect_extra`. Gas accounting stays at its own call sites. `tribute` is not a substitute — it moves `take`, not `save`.

The unchanged contracts settle this as-is: `PoolUpdates._updatePool` subtracts the LP reward total, `Settlement._saveAndSettle` subtracts `save + settle` and requires a zero remaining delta. No encoding change, no new call.

## Round semantics

One snapshot, captured once per round from parent **H**, fixed through matching, gas estimation, and final construction:

```text
canonical H -> DonationSplitSnapshot(H, hash, splits)
  -> solve_pools / BuildProposal -> for_gas_finalization -> process_solution
  -> ValidationRequest::Bundle(parent hash) -> simulate at H, execute at H+1
  -> from_proposal -> process_solution
  -> bundle targeting H+1
```

Never re-read the rate per pool or between estimation and construction. Retain the round's pool snapshots too, rather than re-fetching mutable pool state for final construction.

A setter landing in H+1 before the bundle still does not apply to it; the bundle uses H. This is node policy, not something Angstrom validates.

Identify async work by parent hash plus a round generation that changes on reset, and discard results that no longer match — a matching block height is not enough, since same-height reorgs exist. Simulation must be pinned to the requested parent hash for every read including cache misses; cloning `RethDbWrapper` currently shares an `Arc<AtomicU64>` selector, which defeats this. What that needs is an immutable provider per parent hash, caches that never carry state across hashes, and unavailable state surfacing as an error rather than a fallback to current state — not a wider rework of the provider layer. Submission-time `estimate_gas` needs the same parent state and H+1 environment.

Round reset must **abort** its submission task, not just drop the join handle — a dropped handle leaves the task running. Re-check cancellation and identity after async preparation, before signing, and before each endpoint send.

**Accepted limitation:** a transaction already sent cannot be recalled, and mempool submissions carry no parent-hash condition. One may execute on a replacement branch, or land in a later block if its orders are still valid. Record the construction parent and the actual inclusion parent so the mismatch is visible afterwards. Do not claim stale bundles are excluded.

**Nothing enforces the rates.** Peer finalization compares `PoolSolution`s, which is upstream of where splits are applied, so a leader using a wrong split passes both peer checks and EVM simulation. Correctness here rests on every node running the same release, address, and activation block.

## Payout scope

Fee accounting and reconciliation are required. **Automated payout scheduling is not part of this release** — no scheduled or unattended withdrawal exists. Every distribution is an operator-reviewed timelock execution of the owner-only `ControllerV1.distributeFees`. Nothing built here holds withdrawal authority or initiates a distribution; the fast owner cannot distribute at all.

Before enabling a **nonzero ToB share**, name the accounting component and its responsible operator in the rollout artifacts, and show that it:

1. derives accruals from canonical included bundles, not proposal or submission telemetry;
2. proves a proposed withdrawal leaves LP rewards and user balances backed — Angstrom's ERC20 balance is not the withdrawable amount;
3. undoes and re-derives accruals across reorgs;
4. cannot collect the same fee twice across restart, backfill, or a re-reviewed proposal.

None of this blocks activation at the initial economics, where the ToB protocol share is zero. That is the ordering, not a reduction in scope: the ledger is built and reconciling before step 5 of Rollout, and until then there is no new protocol share for it to account for.

Build it against canonical included bundles. Reconstruct the expected allocations from each bundle's construction parent and the rates in force there, then compare them with the included reward updates and saved amounts. A successful EVM simulation and a passing peer-finalization result are not evidence of compliance — peer checks run on `PoolSolution`s, upstream of where the splits are applied. Report mismatches and missing reconstruction data, and withhold those amounts from any proposed distribution. This detects a bad allocation after inclusion; it cannot prevent or reverse settlement.

Feed it from bundle telemetry recording, per pool and per included bundle: gross ToB payment, LP allocation, explicit protocol fee, allocation residual, and the snapshot identity. Keep the historical construction parent separate from the local round generation so replay and other nodes can reproduce the check.

## Implementation acceptance

Design alignment does not establish implementation correctness. Each item below ships as a named test that fails when the requirement is violated, in the PR that implements it.

1. **One snapshot, one parent.** Drive a round end to end; assert gas estimation and final construction used the same snapshot and parent hash, and that neither re-read config or pool state. Change the head mid-round — including a same-height reorg — and assert the stale result is rejected.
2. **Cancellation.** Invalidate a round while matching, simulation, signing, or an endpoint send is in flight; assert no later send or retry occurs. Assert on sends that did not happen, not on the presence of a token. Cover the dropped-join-handle case.
3. **Conservation.** Per source, `sum(donations) + explicit fee + residual == gross`, all nonnegative, residual attributed to documented allocation steps. Assert the saved amount is both allocated and reserved so `collect_extra` cannot double count it.
4. **Real bundles against real contracts.** Execute builder-produced bundles against unchanged Angstrom in the Anvil harness: exact `save`, zero unresolved deltas, expected reward growth. Hand-written fixtures do not satisfy this.

Coverage that must not be dropped: true no-ops with and without active liquidity; book-only exact-match batches with positive user fees; book no-ops after a ToB move; zero budgets that still carry swap metadata; `Some(empty)`; and **two pools sharing token0**, asserting per-pool application and checked accumulation.

Also test: contract auth (owner, fast owner, everyone else rejected, identical owner/fast-owner, reverting lookups, and authority following a controller replacement), bounds and atomic rejection, ABI shape (exactly one state-changing function, three views, no fallback, no receive, no withdrawal path), getter and slot-0 agreement at one block hash, tracking across startup/commit/reorg/gaps/read failure, `0 / 75 / 80 / 100%` and large-`u128` arithmetic with property tests, and replay behavior either side of **A**.

## Rollout

1. Implement and test the contract, arithmetic, tracking, both splits, allocation, and accounting.
2. Deploy `AngstromProtocolFeeConfig(existingAngstrom, 750_000, 1_000_000)`. Verify resolved authorities, runtime code, layout, initial values, and getter/slot-0 agreement.
3. Configure the address and activation block **A** on all nodes; require the contract to exist in canonical state at **A-1**. A node that cannot read valid config does not build affected bundles.
4. **Activate at the existing economics** (`750_000`, `1_000_000`). Only the config source, the integer arithmetic, and the new allocation paths go live. Verify construction, settlement, and accounting against real blocks.
5. **Then** enable the chosen ToB share via the setter, once step 4 holds and Payout scope is satisfied. Do not combine steps 4 and 5 — a discrepancy would be ambiguous between the code change and the economic change.
6. Replay before **A** keeps the legacy `f64` path, full ToB budget, and legacy allocation behavior. At or after **A**, load rates from historical parent state. Missing historical state is a reported gap, not a silent use of today's rate.
7. To disable the ToB fee, set the pair back to `(currentUserShare, 1_000_000)`. Future rounds only; nothing already accrued reverses.