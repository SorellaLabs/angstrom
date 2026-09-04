# On-chain LP donation splits for user fees and top-of-block auctions

## Objective

Replace the hardcoded user-fee `LP_DONATION_SPLIT` with a value stored in a small on-chain configuration contract, and add an independently configurable LP/protocol split for top-of-block (ToB) auction payments in that same contract. This combines the original user-fee configuration plan with the ToB fee implementation plan; the original authorization, canonical-state tracking, round-snapshot, integer-arithmetic, and rollout requirements remain part of the work.

The contract should expose exactly one external function: a setter callable by either `owner()` or `fastOwner()` of Angstrom's current `ControllerV1` contract. The setter updates both LP shares atomically and emits their old and new values. Each protocol share is the complement of its corresponding LP share.

Deploy only the new configuration contract. The existing `ControllerV1`, Angstrom, and governance contracts require no changes or redeployment.

**Hard deployment constraint:** `OffchainProtocolFeeConfig` is the only contract that this rollout may deploy. No other contract deployment, redeployment, upgrade, or replacement is required or permitted as a dependency of this plan. All remaining implementation changes are off-chain node, simulation, reporting, and operational tooling changes. Existing contracts may be called through their existing interfaces for authorization lookup and fee distribution.

Angstrom nodes should load both values from canonical chain state and pass one fixed, block-specific snapshot through gas estimation and final bundle construction. Simulation must use immutable state from that same parent hash, and pending submission work must be cancellable when its round is invalidated. Apply the ToB split in the bundle builder before ToB rewards merge with book rewards, and retain the protocol portion through the existing `Asset.save` settlement path.

The earlier statement that no contract changes are required still holds for the existing deployed contracts. Storing and administering the parameters on-chain does require deploying the new `OffchainProtocolFeeConfig` contract proposed here. It supplies configuration to nodes; Angstrom does not call it or enforce its ratios during settlement. Enforcing either ratio inside the settlement contract would require additional Solidity validation and is outside this scope.

## Parameters and defaults

| Parameter | Fee base | Initial LP share | Protocol share |
| --- | --- | --- | --- |
| `userLpShareE6` | Fees collected from normal user orders (`total_user_fees`) | `750_000` (75%) | `1_000_000 - userLpShareE6` |
| `tobLpShareE6` | Gross ToB auction payment in token0 (`reward_q`) | `1_000_000` (100%) | `1_000_000 - tobLpShareE6` |

Both parameters apply globally to the pools served by this Angstrom deployment. Bounds are inclusive: `0..=1_000_000`. Per-pool overrides are outside this plan.

Deploy with these defaults to preserve the initial economic policy: 75% of user fees to LPs and no ToB protocol fee. Enable a nonzero ToB protocol share with a later authorized setter call after nodes activate the new flow. For example, `(750_000, 800_000)` keeps the user-fee LP share at 75% and gives LPs 80% of the ToB auction payment, with the remaining 20% retained for the protocol. No production ToB fee percentage is selected by this plan.

## Current behavior

- [`crates/types/primitives/src/contract_payloads/angstrom/mod.rs`](crates/types/primitives/src/contract_payloads/angstrom/mod.rs) defines `LP_DONATION_SPLIT` as `0.75`.
- [`crates/types/src/traits/bundles.rs`](crates/types/src/traits/bundles.rs) applies it in `BundleProcessing::process_solution`:

  ```rust
  let total_lp_user_donate = (total_user_fees as f64 * LP_DONATION_SPLIT) as u128;
  let save_amount = total_user_fees - total_lp_user_donate;
  ```

- Both `from_proposal` and `for_gas_finalization` call `process_solution`. Both paths must receive the same split snapshot.
- [`crates/types/src/traits/tob.rs`](crates/types/src/traits/tob.rs) implements `calc_vec_and_reward`, returning the ToB AMM swap and its surplus in token0. For a token0-input order, that surplus is the order's input minus the AMM input needed for its output. For a token0-output order, it is the AMM output minus the order's requested output. Gas is accounted for separately.
- `process_solution` currently passes the entire ToB surplus into `t0_donation_vec`, then merges the resulting donation vector with book donations. User fees already have a retained portion; ToB surplus has no explicit protocol percentage.
- [`contracts/src/modules/PoolUpdates.sol`](contracts/src/modules/PoolUpdates.sol) consumes encoded LP rewards. [`contracts/src/modules/Settlement.sol`](contracts/src/modules/Settlement.sol) retains `Asset.save`, settles the Uniswap balance, and checks bundle deltas. Neither verifies an LP/protocol percentage.
- [`contracts/src/periphery/ControllerV1.sol`](contracts/src/periphery/ControllerV1.sol) already exposes the owner-authorized `distributeFees` path, which calls Angstrom's `pullFee` and transfers the specified amounts to recipients. Saved fees are not automatically sent to a treasury.

## Configuration contract

Add `contracts/src/periphery/OffchainProtocolFeeConfig.sol` with this proposed implementation:

```solidity
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.26;

import {IAngstromAuth} from "../interfaces/IAngstromAuth.sol";
import {AngstromView} from "./AngstromView.sol";

interface IControllerOwners {
    function owner() external view returns (address);
    function fastOwner() external view returns (address);
}

contract OffchainProtocolFeeConfig {
    using AngstromView for IAngstromAuth;

    IAngstromAuth private immutable ANGSTROM;

    // Packed in storage slot 0, offsets 0 and 4 bytes respectively.
    // 750_000 = 75%; 1_000_000 = 100%.
    uint32 private userLpShareE6;
    uint32 private tobLpShareE6;

    error NotAuthorized();
    error InvalidConfig();

    event LpDonationSplitsSet(
        uint32 oldUserLpShareE6,
        uint32 newUserLpShareE6,
        uint32 oldTobLpShareE6,
        uint32 newTobLpShareE6
    );

    constructor(
        IAngstromAuth angstrom,
        uint32 initialUserLpShareE6,
        uint32 initialTobLpShareE6
    ) {
        if (
            address(angstrom) == address(0) ||
            initialUserLpShareE6 > 1_000_000 ||
            initialTobLpShareE6 > 1_000_000
        ) revert InvalidConfig();

        ANGSTROM = angstrom;
        userLpShareE6 = initialUserLpShareE6;
        tobLpShareE6 = initialTobLpShareE6;
        emit LpDonationSplitsSet(0, initialUserLpShareE6, 0, initialTobLpShareE6);
    }

    function setLpDonationSplits(uint32 newUserLpShareE6, uint32 newTobLpShareE6) external {
        IControllerOwners controller = IControllerOwners(ANGSTROM.controller());
        if (
            msg.sender != controller.fastOwner() &&
            msg.sender != controller.owner()
        ) revert NotAuthorized();
        if (newUserLpShareE6 > 1_000_000 || newTobLpShareE6 > 1_000_000) {
            revert InvalidConfig();
        }

        uint32 oldUserLpShareE6 = userLpShareE6;
        uint32 oldTobLpShareE6 = tobLpShareE6;
        userLpShareE6 = newUserLpShareE6;
        tobLpShareE6 = newTobLpShareE6;
        emit LpDonationSplitsSet(
            oldUserLpShareE6, newUserLpShareE6, oldTobLpShareE6, newTobLpShareE6
        );
    }
}
```

Deploy with constructor arguments `(existingAngstrom, 750_000, 1_000_000)`. This extends the proposed contract before deployment. A previously deployed immutable, single-value configuration contract could not acquire a second field or this new setter; that case would require deploying this configuration contract at a new address and coordinating the address change on nodes. It would still require no changes to existing Angstrom, controller, or governance contracts.

Use the existing [`AngstromView.controller()`](contracts/src/periphery/AngstromView.sol) helper to locate Angstrom's current controller, then check its existing `fastOwner()` and `owner()` getters. Require `msg.sender` to equal either address on every setter call. This matches the caller permissions of `ControllerV1._checkFastOwner()` and makes authorization follow the current controller without storing separate owners or adding ownership-transfer functions to the configuration contract.

`IControllerOwners` describes the existing controller getters; it does not add getters or other external functions to `OffchainProtocolFeeConfig`.

Keep both shares and the immutable Angstrom reference private to preserve exactly one externally callable function. Rust can read storage directly. A public state variable would introduce an automatically generated getter. There are no inherited external functions, fallback, receive, ownership-transfer, or withdrawal functions on `OffchainProtocolFeeConfig`.

The immutable Angstrom reference consumes no storage slot. The two `uint32` shares are packed into slot zero, with the first field in its least-significant four bytes. Treat this layout as part of the contract's interface and verify it against compiler storage-layout output. See the [Solidity storage-layout documentation](https://docs.soliditylang.org/en/v0.8.26/internals/layout_in_storage.html) and [contract documentation](https://docs.soliditylang.org/en/v0.8.26/contracts.html).

```text
word = storage[0]
user_lp_share_e6 = word & 0xffff_ffff
tob_lp_share_e6  = (word >> 32) & 0xffff_ffff
```

One storage read obtains the complete pair. Validate both decoded values and the expected zero padding above bit 63. Updating one economic setting still requires passing both values; governance tooling should display both explicitly and re-check the other value before execution, especially for delayed timelock operations. A queued call deliberately writes its complete pair and can otherwise overwrite an intervening fast-owner update.

## Owner and fast-owner authorization

Either the owner or fast owner of [`ControllerV1`](contracts/src/periphery/ControllerV1.sol) calls `OffchainProtocolFeeConfig.setLpDonationSplits` directly. Each is independently authorized; joint approval is not required. No forwarding function or configuration reference is added to `ControllerV1`.

The authorization flow is:

```text
AngstromView.controller(ANGSTROM)
  -> current ControllerV1 address
  -> ControllerV1.fastOwner() or ControllerV1.owner()
  -> either address may be msg.sender for OffchainProtocolFeeConfig.setLpDonationSplits
```

The controller contract and the configuration contract's deployer receive no independent permission to update the split. A caller is authorized only if its address equals the value returned by the current controller's `owner()` or `fastOwner()` getter.

If an authorized address is a contract, that contract must execute the call. The mainnet deployment path in [`contracts/script/Angstrom.s.sol`](contracts/script/Angstrom.s.sol) sets a timelock as `owner` and the Angstrom multisig as `fastOwner`. For that setup, the multisig can call `OffchainProtocolFeeConfig` directly without the timelock delay. The timelock can also call it through its existing scheduling and execution process. A proposer or signer address is authorized directly only if it is itself one of the two returned addresses.

There are no independently transferable owners on `OffchainProtocolFeeConfig`; its administrative authority is always derived from the current controller. If Angstrom's controller is replaced in the future, authorization follows the replacement's `owner()` and `fastOwner()` getters. The replacement must support those getters for both authorization paths to remain available. No controller replacement is required for this deployment.

Configuration authority and fee-withdrawal authority remain separate. The fast owner may change either split, but `ControllerV1.distributeFees` remains owner-only. Setting a new split neither grants the fast owner withdrawal rights nor changes the recipient of existing payouts.

## Rust integration points

| Location | Change |
| --- | --- |
| [`crates/types/primitives/build.rs`](crates/types/primitives/build.rs) | Add `OffchainProtocolFeeConfig.sol` to `WANTED_CONTRACTS` and regenerate the ABI and Rust bindings. |
| [`crates/types/constants/src/lib.rs`](crates/types/constants/src/lib.rs) | Add the configuration contract address and coordinated activation information to network configuration and builders. Keep the changing shares out of `OnceLock` constants. |
| Shared types under [`crates/types/primitives/src`](crates/types/primitives/src) and a configuration-tracking module under [`crates/eth/src`](crates/eth/src) | Define validated `DonationSplits` and `DonationSplitSnapshot` types; isolate storage decoding, code/layout checks, and canonical-state loading in one module. |
| [`bin/angstrom/src/components.rs`](bin/angstrom/src/components.rs) | Initialize tracking at the exact block used to initialize the node, then inject the tracking state into the block and consensus flow. |
| [`crates/eth/src/manager.rs`](crates/eth/src/manager.rs) | Refresh both shares on canonical commits and reorgs before releasing the corresponding block update to consumers. Decode configuration events for history and telemetry. |
| [`crates/consensus/src/manager.rs`](crates/consensus/src/manager.rs) and [`crates/consensus/src/rounds/mod.rs`](crates/consensus/src/rounds/mod.rs) | Capture one immutable snapshot of both shares for each round and pass it into matching and proposal construction. Preserve gross ToB auction ranking. |
| [`crates/matching-engine/src/lib.rs`](crates/matching-engine/src/lib.rs) and [`crates/matching-engine/src/manager.rs`](crates/matching-engine/src/manager.rs) | Carry the snapshot through `MatchingEngineHandle::solve_pools`, `MatcherCommand::BuildProposal`, and gas-finalization bundle construction. |
| [`crates/validation/src/bundle/validator.rs`](crates/validation/src/bundle/validator.rs), [`validator.rs`](crates/validation/src/validator.rs), and [`bundle/mod.rs`](crates/validation/src/bundle/mod.rs) | Carry parent identity and round generation through `ValidationRequest::Bundle`, simulation, and the gas response; execute against immutable parent state with target block H+1. |
| [`crates/types/src/reth_db_wrapper.rs`](crates/types/src/reth_db_wrapper.rs) and [`reth_db_provider.rs`](crates/types/src/reth_db_provider.rs) | Provide hash-pinned state for each simulation and configuration read. Avoid shared mutable block selectors and propagate unavailable-state errors instead of panicking. |
| [`crates/consensus/src/rounds/proposal.rs`](crates/consensus/src/rounds/proposal.rs) and [`crates/types/src/submission`](crates/types/src/submission) | Reuse the round's pool and configuration snapshots for final construction; cancel pending submission work on invalidation; check round identity before signing and each endpoint send; address submission-time gas estimation. |
| [`crates/types/src/traits/bundles.rs`](crates/types/src/traits/bundles.rs) | Add explicit validated split input to `from_proposal`, `for_gas_finalization`, and `process_solution`; replace the user-fee floating-point calculation; split gross ToB rewards before merging; extend retained-fee accounting. |
| [`crates/types/src/traits/tob.rs`](crates/types/src/traits/tob.rs) | Keep gross auction-payment calculation and swap quantities intact. Document the distinction between gross payment and LP reward; propagate failures for selected orders instead of silently treating them as zero ToB revenue. |
| [`crates/types/src/uni_structure/pool_swap.rs`](crates/types/src/uni_structure/pool_swap.rs) and [`donation.rs`](crates/types/src/uni_structure/donation.rs) | Verify reduced ToB donation budgets, tick-range merging, zero-share handling, and explicit rounding reconciliation. Preserve the existing allocation policy. |
| [`crates/types/primitives/src/contract_payloads/asset/builder.rs`](crates/types/primitives/src/contract_payloads/asset/builder.rs) and [`state.rs`](crates/types/primitives/src/contract_payloads/asset/state.rs) | Reuse allocation plus `save` accounting; verify no double counting with `collect_extra`. A clearer internal saved-fee helper may wrap the existing misleadingly named `add_gas_fee`; no asset encoding change is needed. |
| [`crates/types/primitives/src/contract_payloads/angstrom/mod.rs`](crates/types/primitives/src/contract_payloads/angstrom/mod.rs) | Remove the hardcoded `LP_DONATION_SPLIT`. |
| New standalone configuration deployment script and [`testing-tools`](testing-tools) | Deploy `OffchainProtocolFeeConfig` against the existing Angstrom address; update address initialization, test harnesses, and replay setup. The existing full Angstrom deployment script is not required for this rollout. |
| Bundle telemetry, historical fee reporting, and the fee-accounting/payout component identified during implementation | Record gross ToB payment, LP allocation, explicit protocol fee, rounding residual, and snapshot identity. Reconstruct canonical included allocations and reconcile protocol proceeds to `Asset.save` and the controller's existing distribution calls. Integrate an identified external ledger or implement this component; its existence is not assumed. |

Resolve all callers of the changed bundle/matching interfaces, including mocks, benchmark helpers, testnet setup, and replay. Update the relevant bundle-building documentation and add an operational procedure for reading slot zero and submitting the sole setter. Updating `PLAN.md` itself does not implement or deploy any of these changes.

## Tracking canonical state

Read storage slot zero, containing both shares, at each canonical head using the local provider and an explicit block hash. Keep read logic and validation in the configuration-tracking module. An RPC-backed implementation can use `eth_getStorageAt` with an EIP-1898 block-hash identifier and `requireCanonical: true` for live construction. Require support for hash-pinned state reads; do not substitute an unpinned `latest` or block-number-only read. See the [Ethereum JSON-RPC documentation](https://ethereum.org/developers/docs/apis/json-rpc/#eth_getstorageat) and [EIP-1898 block-hash identifiers](https://eips.ethereum.org/EIPS/eip-1898).

Represent a snapshot with at least:

```rust
struct DonationSplits {
    user_lp_share_e6: u32,
    tob_lp_share_e6: u32,
}

struct DonationSplitSnapshot {
    block_number: u64,
    block_hash: B256,
    splits: DonationSplits,
}
```

Only construct usable snapshots after validating both shares are in `0..=1_000_000`. Keep the fields behind validated constructors/accessors and provide a shared integer split operation. Bundle-building code should receive validated values, not perform storage reads or accept unrelated raw rates. Preserve parent-block identity through asynchronous work even where a pure helper only needs `DonationSplits`.

The tracking flow must satisfy these rules:

1. Subscribe to canonical updates before taking the startup snapshot, then reconcile queued updates so startup cannot miss a change.
2. Verify the configured address contains the expected contract code, tied to the intended immutable Angstrom address, and validate both loaded shares and the layout. Account for constructor-patched immutables when checking runtime bytecode. An empty account returning zero storage must not be mistaken for two valid 0% settings; an old one-field configuration contract must not be mistaken for a zero ToB LP share.
3. On a commit, read both shares from one word at the committed head and publish them with that block's identity.
4. On a reorg, read the replacement head's state, including when the replacement branch contains no split-change event.
5. Complete the read before allowing consumers to build the corresponding round. The current cleanser callbacks are synchronous, so asynchronous reads must participate in block synchronization rather than run as detached updates.
6. Do not build a round with stale configuration when a required read fails. Retry or skip the affected round according to the node's error-handling policy. The current local-provider adapter unwraps state-provider acquisition errors for code and storage reads; bypass or replace those panic paths so unavailable, pruned, or reorged state reaches this error policy.
7. Handle notification gaps by resynchronizing with canonical state before resuming affected rounds.

Keep `LpDonationSplitsSet` events for change history and telemetry, filtering by the configuration contract's address. Receipt and log metadata provide the block hash, transaction, and ordering information. An event-history consumer must account for removed blocks and process all relevant blocks in a notification. The pair in canonical storage is authoritative; events are not a second independently maintained configuration source.

Do not maintain the effective value solely by copying the existing periphery event handler. It currently scans only the tip's receipts, and its reorg path applies new logs without undoing old configuration changes. Reading canonical storage handles an update that is removed by a reorg even if no replacement update exists.

## Round semantics and propagation

Use both shares from parent block **H** for a bundle targeting **H+1**. An update included in H affects construction for H+1. It does not retroactively change an in-flight bundle built against H-1. Even if a setter transaction executes before the Angstrom bundle within H+1, the bundle uses H's snapshot; the setter affects construction for H+2. This is an explicit node policy, not execution-time validation by Angstrom.

Capture the snapshot once for the round, after the relevant block state is ready, and keep it fixed throughout matching, gas estimation, and final bundle construction. Retain the corresponding pool snapshots rather than fetching mutable pool state again for final construction. Identify asynchronous work by parent number, parent hash, and a local round generation that changes on reset. A new head or reorg must invalidate pending work from the previous round, subject to the already-submitted transaction limitation below.

Pass the same split through both paths:

```text
Canonical state at H
  -> DonationSplitSnapshot(H, hash, { user_lp_share_e6, tob_lp_share_e6 })
  -> Consensus round with fixed pool snapshots and round generation
       -> solve_pools / BuildProposal
            -> for_gas_finalization
                 -> process_solution
            -> ValidationRequest::Bundle(parent identity, round generation)
                 -> immutable simulation state at H; EVM target H+1
                 -> gas result carrying the same identity
       -> from_proposal
            -> process_solution
       -> cancellable submission with identity checks
  -> Bundle targeting H+1, recorded with its construction parent
```

Do not independently read a mutable global value inside each pool's processing or between gas estimation and final construction. Pass the validated split explicitly into the bundle functions.

Check that configuration and pool snapshots refer to the intended same parent state before starting and before submitting the result. Discard local results from invalidated rounds, including same-height reorgs. A matching height alone is insufficient.

### Simulation state

Extend `BundleValidatorHandle::fetch_gas_for_bundle` and `ValidationRequest::Bundle` to carry the expected parent identity and round generation. The validator currently selects its own mutable height, and cloning `RethDbWrapper` retains a shared `Arc<AtomicU64>` block selector. Passing a fee snapshot to the bundle builder does not freeze this database.

Acquire an immutable state provider pinned to the requested parent hash for each simulation. All account, code, and storage reads, including cache misses, must use that provider; caches must not carry state between different parent hashes. Set the EVM execution block to H+1 while reading parent state H. Return the parent identity and round generation with the gas result, and reject results that no longer match the active round. Failures to acquire the requested state must produce an error, not a fallback to current state.

The submission layer also calls `estimate_gas` without a block selector today. Its gas-limit calculation must use a simulation of the final transaction against the same parent state and intended target-block environment, or reuse a verified estimate that covers that final transaction. An RPC implementation must support those state/environment semantics; merely selecting block H does not set the EVM execution block to H+1. Do not silently use an unpinned RPC estimate when this support is unavailable. Network fee suggestions are separate from the LP/protocol split and simulation-state identity.

### Submission cancellation and reorg limits

Give pending matching, simulation, signing, and submission work the round's cancellation context. Round reset must cancel or abort owned submission tasks; replacing the round state alone is insufficient because the current spawned task continues after its join handle is dropped. Computation that cannot stop immediately must have its output discarded and must not trigger later signing or submission. Check cancellation and parent/generation identity after asynchronous preparation, immediately before signing, and immediately before each endpoint send or retry. Check again inside each endpoint task so queued sends cannot continue after invalidation.

Cancellation stops pending local work; it cannot recall a transaction already sent or guarantee cancellation of an in-flight network request. Existing mempool submissions have no parent-hash condition, and existing MEV submissions select a target block number only. This rollout explicitly accepts that a transaction already submitted before a same-height reorg may execute on the replacement branch using its original construction snapshot. A mempool transaction may also land in a later block when its orders remain valid, such as a bundle containing only standing orders. Record both the construction parent and actual inclusion block/parent, detect and report any resulting policy mismatch, and reconcile actual settled amounts. Do not promise unconditional stale-bundle exclusion with unchanged contracts. Stronger exclusion is outside this plan; it must not introduce another contract deployment or an existing-contract change into this rollout. Replacement/cancellation requests alone are not a guarantee.

### Policy verification scope

Current peer finalization compares `PoolSolution` values, while the splits are applied later in bundle construction. Therefore neither Angstrom settlement nor existing peer verification enforces these shares, and a leader using an incorrect split can pass both checks. This rollout requires correct local construction, coordinated node activation, and post-settlement allocation verification in fee reporting; it does not add consensus-level split enforcement, network-message changes, or slashing.

The fee-accounting component must reconstruct the expected allocations using the included bundle's canonical parent and the applicable activation policy, then compare them with actual included reward updates and saved amounts. Do not trust a leader-supplied rate or treat a successful EVM simulation or peer-finalization result as proof of compliance. Report mismatches or unavailable reconstruction data and withhold the affected amounts from automated payout until reconciled. This check detects incorrect allocations after inclusion; it cannot prevent or reverse settlement. Preserve historical parent identity separately from local round generation so replay and other nodes can reproduce the check.

## Integer calculation

Use integer parts per million and a `U256` intermediate for both fee bases. The following is illustrative; the implementation should put the arithmetic behind the validated `DonationSplits` interface:

```rust
fn split_amount(gross: u128, validated_lp_share_e6: u32) -> (u128, u128) {
    let lp = (
        U256::from(gross)
        * U256::from(validated_lp_share_e6)
        / U256::from(1_000_000u32)
    ).to::<u128>();
    (lp, gross - lp)
}

let (total_lp_user_donate, user_protocol_fee) =
    split_amount(total_user_fees, splits.user_lp_share_e6);
let (tob_lp_budget, tob_protocol_fee) =
    split_amount(gross_tob_reward, splits.tob_lp_share_e6);

let save_amount = user_protocol_fee
    .checked_add(tob_protocol_fee)
    .ok_or_else(|| eyre::eyre!("retained fees exceed the asset amount range"))?;
```

The LP calculation rounds down and the protocol receives the exact remainder, so `lp + protocol == gross` for each fee base. With a validated share no greater than `1_000_000`, LP rewards cannot exceed that base, and the intermediate multiplication fits in `U256`. Use checked additions when accumulating amounts across fee types and pools; reject a bundle that cannot be represented in its existing `u128` fields rather than saturating or wrapping a fee.

This also removes the existing floating-point precision loss. Even at 75%, large quantities may round differently from the old implementation. Treat the arithmetic change and the new no-op/allocation-error behavior as part of the coordinated activation. Byte-exact pre-activation replay must retain the legacy builder behavior for both, not just the old percentage calculation.

## ToB fee application and LP allocation

Implement the split in `BundleProcessing::process_solution`, after `calc_vec_and_reward` has returned the gross ToB payment and before building `tob_donation_vec`.

1. Keep the returned swap vector, order quantities, AMM movement, and post-ToB price unchanged. The fee divides value the auction already pays; it does not levy an additional charge on the searcher or change its signature. Do not repurpose `gas_used_asset_0`, which represents separately bounded gas charges.
2. Set `gross_tob_reward` to the returned `reward_q`. With no selected ToB order, set both the ToB LP budget and protocol fee to zero. If a selected ToB order cannot be evaluated against the round's pool state, propagate the error rather than proceeding with that order and zero assumed revenue.
3. Calculate `(tob_lp_budget, tob_protocol_fee)` using the validated ToB LP share. Keep `calc_vec_and_reward`, `calc_reward`, and auction-bid calculations expressed in gross token0 payment; changing their meaning would affect other callers and obscure the auction accounting.
4. Call `tob_vec.t0_donation_vec(tob_lp_budget)` in place of the current call using the full gross payment. The choice in this plan is to run the existing range-allocation policy with the reduced LP budget. This may change relative rewards between tick ranges. It is not a promise that each individual LP receives exactly the configured fraction of its previous claim. That different policy would require scaling the original full-reward ToB vector before merging, with a separate rounding specification.
5. Build book donations using `solution.reward_t0 + total_lp_user_donate` as before. The ToB parameter must not apply to user fees, book-matching surplus, gas reimbursements, or unlocked-swap fees. The user-fee parameter continues to affect only `total_user_fees`.
6. Merge the reduced ToB donation vector with the book donation vector using `DonationCalculation` in the existing order. Compute `total_donation` from the actual merged donations and encode the existing `RewardsUpdate` and `PoolUpdate` types, including the optional second reward update.
7. Add `tob_protocol_fee` to the existing retained user-fee amount, then use the asset-accounting flow below.

Apply a share once per pool's total gross ToB payment, not per tick, reward update, order fragment, or asset after multiple pools have been aggregated. Keep the gross auction-ranking policy and its existing tie-breaker. With a common rate for a pool, reducing the LP share does not require selecting a different winning bid; ranking gross amounts also avoids introducing new rounding ties.

### Donation rounding and zero-share cases

The share calculation conserves its input exactly, but the existing range allocator and on-chain reward-growth arithmetic have their own rounding. These are separate from rounding the percentage.

- Return or expose the allocator's final unallocated amount alongside its donation vector. Independently reconcile ToB's `tob_lp_budget` and the book budget `solution.reward_t0 + total_lp_user_donate` before merging: checked `sum(source_donations) + source_residual == source_budget`. Preserve the existing policy of retaining valid integer-allocation residuals through `collect_extra`, and report them separately from explicit percentage fees. Do not assume the nominal budget always equals the encoded donation total.
- Verify `encoded_tob_lp_donation + tob_protocol_fee + tob_allocation_residual == gross_tob_reward`, with all values nonnegative, and verify that merging and encoding preserve the combined source donation total. Classify a residual as allocation rounding only after validating the source path and recipients and attributing the remainder to the documented integer-price allocation steps, including existing boundary guards. Document and test that arithmetic justification; budget-minus-donations alone does not prove correct allocation. Reject over-allocation, skipped or malformed ranges, arithmetic failures, and unexplained remainders regardless of size. Do not use an arbitrary "material" threshold or assume a universal one-unit rounding bound.
- Apply a deterministic no-op rule to both sources. A true no-op has zero token deltas and unchanged price and tick. Allocate its entire positive budget to the active range at that source's end state, with zero allocation residual, and reject construction if that range has no positive liquidity. This includes book-only exact-match batches with positive user fees and book no-ops after a ToB swap; the latter use the post-ToB state rather than the original parent pool price. A moving swap with missing or inconsistent range metadata is an allocation error, not a no-op fallback. Require positive liquidity for every positive donation. Do not let `Some(empty)` bypass the current-range allocation and silently retain LP user fees or ToB rewards.
- A zero budget from either source produces zero donations and zero residual while preserving required swap and tick/liquidity metadata; it does not excuse an invalid swap path. At `tobLpShareE6 == 0`, the ToB order and any book rewards still settle normally. At `1_000_000`, the explicit ToB protocol fee is zero and the established valid ToB donation behavior remains the baseline, subject to the explicit no-op/error rules above.
- Existing fixed-point rounding in LP claims is not an additional protocol fee and must not be swept as treasury revenue. The configuration change does not alter existing accrued LP rewards.

## Retained-fee accounting and treasury distribution

Reuse the same accounting pattern already used for retained user-order fees:

```rust
// total_donation includes actual LP allocations from both sources.
// save_amount = user_protocol_fee + tob_protocol_fee, checked above.
asset_builder.allocate(AssetBuilderStage::Reward, t0, total_donation);
asset_builder.allocate(AssetBuilderStage::Reward, t0, save_amount);
asset_builder.add_gas_fee(AssetBuilderStage::Reward, t0, save_amount);
```

Despite its name, `add_gas_fee` increments the saved amount; the existing user-fee code already uses it for non-gas fees. Reserve the saved amount as well as incrementing `save`, so it is not left as free liquidity for `collect_extra` to count again. Keep separate gas accounting at its existing call sites. Do not use `tribute` as a substitute: its current implementation changes `take`, not `save`. Recompute asset arrays through the existing builder rather than manually changing only an encoded `save` field.

The unchanged contracts can settle this allocation:

```text
Gross ToB payment in token0
  -> LP donation -> PoolUpdate / RewardsUpdate -> LP reward growth
  -> protocol fee -> Asset.save -> tokens retained in Angstrom
                               -> owner calls ControllerV1.distributeFees
                               -> Angstrom.pullFee -> treasury recipient
```

`PoolUpdates._updatePool` subtracts the encoded LP reward total from the bundle delta. `Settlement._saveAndSettle` subtracts `save + settle`, requires the remaining delta to be zero, and returns the settlement amount to Uniswap. Moving the explicit ToB protocol portion from LP allocation into `save` fits this existing accounting. No new token transfer, order field, pool-update field, reward decoder, or settlement-contract call is needed per bundle.

`Asset.save` aggregates gas, retained user fees, the new ToB protocol fee, and any existing residual retention. It has no source-specific sub-balances or automatic treasury routing. The fee-accounting component must track the explicit ToB fee per pool, token, included bundle, and parent snapshot, aggregate by token for collection, and reconcile the result against actual saved amounts and the existing fee-summary log commitment. Account for rounding residuals separately and document the distribution policy for other fee sources.

The repository provides the owner-only withdrawal mechanism, but no source-specific fee ledger or payout implementation has been identified here. Treat this as an explicit implementation dependency: identify and integrate an external component, recording its repository/service, release, responsible operator, and operating procedure in the rollout artifacts, or implement the required ledger and reporting tooling as part of this work. Before enabling a nonzero ToB protocol share, that component must ingest canonical included bundles and required historical parent data, perform the allocation checks above, and demonstrate reconciliation and duplicate-collection prevention.

Establish a reconciled ledger checkpoint through A-1 before activation, keyed by canonical block number/hash and token. Record accrued, already distributed, and uncollected amounts from existing fee sources; the new ToB percentage-fee balance starts at zero. Historical unresolved amounts must remain separately identified and excluded from automated collection. Persist accrual and distribution identities so restart, backfill, or reorg processing cannot count them twice; advance the checkpoint with canonical included data rather than proposal or submission telemetry alone.

Use the owner's existing `ControllerV1.distributeFees` call to withdraw only reconciled, uncollected fees and direct the ToB component to the protocol treasury. `pullFee` trusts the controller's requested amount rather than maintaining a new accrued-ToB balance. Do not infer withdrawable fees from Angstrom's entire ERC20 balance, which also backs LP rewards and internal user balances. Record completed distributions and undo/reconcile records affected by reorgs to avoid collecting the same fee twice.

The configuration contract holds only the two rates and no fee funds. Treasury selection and payout execution belong to the identified accounting/operational workflow using the existing controller withdrawal function; adding a treasury setter or payout function to the configuration contract would violate the one-external-function requirement and is unnecessary here.

## Validation

- Contract: both initial values and the initialization event; independent acceptance of the current controller's owner and fast owner; rejection of all other callers, including the controller and deployer when they match neither authorized address; bounds on each parameter; atomic rejection if either value is invalid; correct old/new event values; changes to either or both values; unchanged-value updates.
- Contract interface: compiled ABI has exactly one externally callable function, `setLpDonationSplits(uint32,uint32)`, with no getters or fallback/receive entry points. No existing contract ABI or runtime bytecode needs to change.
- Governance: a timelock owner successfully executes a scheduled setter call; a multisig fast owner successfully calls the setter directly without a timelock operation; a proposer or signer matching neither authorized address is rejected.
- Authority lookup: updates use the current controller's `owner()` and `fastOwner()` getters; after a controller replacement, both new authorities are accepted and previous authorities are rejected unless they retain either role. Cover identical owner/fast-owner addresses, an unset fast owner with a valid owner, and reverting required lookups without permissive fallback.
- Storage: verify both field offsets in slot zero and Rust decoding against the compiled contract. Cover zero values, invalid values, nonzero padding, missing code, mismatched immutables, and a one-value config contract at the configured address.
- Arithmetic: 0%, 75%, 80%, 100%, fractional rounding, and large `u128` fee amounts for both sources. Property-test `lp + protocol == gross`, monotonicity, and bounds. Exercise checked overflow when aggregating multiple sources or pools into an asset.
- Tracking: startup after an earlier update, an update during startup, multiple updates in one block, multi-block notifications, notification gaps, and read failures. Verify both parameters always originate from the same storage word and parent hash.
- Reorgs: removal of an update with no replacement event, replacement with another value, and different block hashes at the same height.
- Bundle integration: confirm gas estimation and final construction use the identical snapshot of both rates, including a head change during matching, an invalidated asynchronous result, and an update before the bundle within its target block. Confirm selected orders with invalid ToB reward calculations cannot be silently included with zero assumed revenue.
- Simulation: change the canonical head during uncached account/code/storage reads and during submission-time gas estimation, including a same-height reorg. Verify that each simulation uses one immutable parent state, executes at H+1, and returns its parent/generation identity. Verify stale-result rejection, cache isolation, and recoverable unavailable-state errors instead of shared-block reads, unpinned RPC fallback, or panics.
- Submission: invalidate a round while nonce/gas RPCs, signing, or endpoint sends are queued; assert cancellation prevents subsequent local sends and retries. Separately cover a transaction already sent before a same-height reorg and a standing-order-only mempool transaction included in a later block: record the original construction parent and actual inclusion parent, detect any split mismatch, and reconcile actual settlement without claiming the transaction was recalled.
- Policy verification: construct identical `PoolSolution`s with different fee splits and demonstrate that current peer verification and EVM success do not establish compliance. Verify that post-settlement reconstruction detects incorrect reward/save allocations, accepts correct allocations, and excludes mismatches or missing-data cases from automated payout pending reconciliation.
- ToB allocation: token0-input and token0-output orders; ToB-only, book-only, and mixed bundles; current-only and multi-tick rewards; opposing ToB/book swaps and the optional second reward update; no-op/empty vectors; positive rewards without eligible liquidity; and zero LP/protocol shares. Verify ToB allocation plus explicit fee plus identified allocation residual conserves the gross ToB payment.
- Allocation edge cases: book-only exact-match batches with positive user fees; book no-ops after a ToB move; both sources' true no-ops with and without positive active liquidity; moving swaps with missing ranges; and zero budgets with required metadata preserved. Assert the exact current-range allocation or error prescribed above. Verify allocator-reported residuals against the documented arithmetic, and source-total preservation through merging and encoding.
- Isolation: changing the ToB rate leaves user-fee splitting, book surplus, gas charges, searcher settlement amounts, gross bid ranking, and AMM swap quantities unchanged. Changing the user rate leaves the ToB rate unchanged. Compare the default 100% ToB LP setting to existing valid ToB bundle fixtures, accounting separately for the planned user-fee integer change and explicit invalid-path handling.
- Settlement: execute generated bundles against the existing Angstrom contracts in the Anvil/Foundry harness. Assert exact saved amounts with gas and residuals accounted for once, zero unresolved deltas, and expected LP reward growth/claims subject to existing fixed-point rounding. Include multiple pools sharing token0.
- Treasury: exercise the identified or newly implemented accounting component, its A-1 opening checkpoint, canonical included-bundle ingestion, restart/backfill, and reorg corrections. Distribute the new proceeds to a treasury through the existing owner-only controller function; show that LP and user liabilities remain backed. Verify a fast owner that is not also owner cannot distribute fees. Reconcile fee-summary data, prevent duplicate collection, and exclude unresolved historical amounts and policy mismatches from automated payouts.
- Replay: reconstruct both shares from the correct historical parent state and apply an explicit policy for blocks predating deployment or activation. When reproducing historical bundle bytes, preserve both the old floating-point user-fee calculation and the legacy allocation/no-op/error behavior. Test the behavior boundary at A.

Run the focused Rust and Foundry tests for the new configuration contract, arithmetic, tracking, and bundle execution, then the repository-required checks for implementation changes. Regenerate and verify bindings and compiler storage layout during implementation. For this planning-only update, verify document consistency and referenced paths; implementation tests and deployment are not performed.

## Implementation sequence

1. Define the validated pair of shares and its arithmetic, then implement `OffchainProtocolFeeConfig`, its one setter, contract tests, ABI bindings, and a standalone deployment script. Verify the storage interface before implementing node decoding.
2. Implement hash-pinned canonical-state loading and block-synchronized snapshot publication, including recoverable provider errors. Wire startup, commits, reorgs, gaps, and failures before allowing rounds to consume configuration.
3. Thread one snapshot and round identity through consensus, matching, immutable-state simulation, final bundle construction, and cancellable submission. Address submission-time gas estimation and already-submitted reorg handling. Replace the user-fee constant and apply the ToB split and asset accounting at the shared `process_solution` entry point.
4. Implement deterministic no-op allocation and explicit residual reporting. Identify and integrate the external fee-accounting component or implement it, including post-settlement policy verification and ledger initialization. Update test/replay/address initialization and validate full bundles against the unchanged settlement contracts. Keep this as one coordinated behavior change across construction paths rather than enabling one path early.
5. Prepare the deployment and activation artifacts: constructor arguments, verified runtime/layout, network address, deployment block/hash, activation target block, node release, fee-accounting component/operator, opening-ledger procedure, and governance execution and treasury distribution procedures. Record the limits of peer verification and already-submitted transaction cancellation.

## Rollout

1. Implement and validate the configuration contract, owner and fast-owner lookups, bindings, canonical tracking, both split calculations, deterministic allocation, retained-fee accounting, immutable-state simulation, and cancellation through submission. Identify or implement the fee-accounting component and validate its historical-data and reconciliation workflow.
2. Deploy only `OffchainProtocolFeeConfig` with constructor arguments `(existingAngstrom, 750_000, 1_000_000)`. Verify both resolved controller authorities and their existing execution paths, runtime code, storage layout, and initial values. No controller handover or existing-contract redeployment is needed.
3. Configure the same contract address and activation target block **A** on all participating nodes and update deployment/test tooling. Require the contract to exist in canonical parent state A-1. Nodes unable to obtain valid configuration at activation must not construct affected bundles with a fallback rate.
4. Establish the fee ledger's reconciled A-1 checkpoint and coordinate the transition to chain-derived configuration and integer arithmetic for bundles targeting A and later. Keep initial LP shares at 75% for user fees and 100% for ToB during activation. Verify all participating nodes use the intended release/address/activation settings; existing peer checks cannot establish rate agreement. Observe agreement between simulation, construction, post-settlement allocation checks, saved-fee reports, and settlement before enabling a ToB fee.
5. For replay of targets before A, retain the legacy user-fee floating-point behavior, full ToB LP budget, and legacy allocation/no-op/error behavior when byte-exact reproduction is required. For targets at or after A, load both shares from their actual historical parent state and apply the new integer and allocation behavior. Deployment alone does not activate the new node policy. If a replay lacks required historical state, report the gap rather than silently applying today's setting.
6. Have either the existing controller's owner or fast owner call `setLpDonationSplits(currentUserShare, chosenTobShare)` once participating nodes use the new flow. A multisig fast owner can execute the call directly; a timelock owner uses its existing scheduling and execution process with `OffchainProtocolFeeConfig` as the target. Read and review both supplied values to avoid resetting an unrelated share. The final values at the setter's canonical block H govern construction for H+1.
7. Reconcile retained ToB fees in the identified fee-accounting/payout component and have the controller owner distribute them to the chosen treasury. Exclude unreconciled policy mismatches or missing historical data from automated collection. Changing the rate does not perform a payout.
8. To turn off the ToB fee later, call the same setter with the current user share and `1_000_000` for the ToB LP share. This affects future parent snapshots; it neither reverses already accrued fees nor requires redeployment. An unavailable configuration read remains a synchronization failure, not a reason to silently roll back rates.

No bundle encoding change is required to source either split from chain state or to retain the ToB protocol portion: the resulting donation and saved amounts already enter the encoded bundle. The new contract configures off-chain policy, with post-settlement checks in the accounting workflow. Consensus-level enforcement, additional on-chain enforcement of the ratios, guaranteed recall of submitted transactions, automatic treasury forwarding, and changes to existing Angstrom/controller/governance contracts are outside this plan.
