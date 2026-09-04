# Exact protocol-fee accounting and collection

Start every cumulative ledger at the deployment's exact CREATE prestate, or at a previously reviewed
checkpoint cryptographically chained to it. End it at an explicit block number and hash; never store
`latest` as an accounting boundary.

## Non-negotiable rule

Angstrom has two separate collection paths:

| Fee source | Where it is held | Collection call |
|---|---|---|
| Unlocked-swap `protocolUnlockedFee` | PoolManager ERC-6909 claims owned by `FEE_COLLECTOR` | `ControllerV1.collect_unlock_swap_fees(to, packed_assets)` |
| Bundle/book fees | Commingled in Angstrom with internal user balances, LP-designated value, and incidental value | `ControllerV1.distributeFees(assets)` -> `Angstrom.pullFee` |

Never use Angstrom's token balance, `balance - grossSave`, or gross `save` as a withdrawal amount.
The current audit authorizes **no nonzero bundle-held withdrawal**. The collector path is usable only
after the live checks below.

Native ETH held by Angstrom is incidental, not a bundle fee, and `pullFee(address(0), amount)` is not
an ETH-withdrawal path. Native unlocked fees exist only as collector currency ID `0`.

## Mainnet endpoints

```text
Angstrom:    0x0000000aa232009084Bd71A5797d089AA4Edfad4
Controller:  0x1746484EA5e11C75e009252c102C8C33e0315fD4
PoolManager: 0x000000000004444c5dc75cB358380D2e3dE08A90
Collector:   0x59a82241ce490fB77370e3f614de04Aa188e13d2
Owner:       0x60D41d9708BBEfd29000d1486C6406Ef23526c01  (Timelock)
Fast owner:  0xD31C82069da3013fdB16B731AD19076Af9b93105  (Safe)
```

At audited block `25,898,173`, the Timelock delay was five days and execution of ready scheduled
operations was open. These values are not permanent. Verify chain ID, runtime hashes, immutables,
controller, owner, `fastOwner`, pending controller, Timelock roles, Safe configuration, and delay
again before every proposal or execution.

## 1. Collect unlocked-swap protocol fees

1. From PoolManager deployment onward, build the currency-ID universe from every ERC-6909 `Transfer`
   to or from the collector, including transfers before the collector was created. For each ID initialize:

   ```text
   protocolClaim = 0
   incidentalClaim = preCreationClaimBalance
   ```

2. Replay every post-creation claim event in execution order:

   ```text
   protocolClaim += amount
     only when caller=Angstrom, from=0, to=Collector, and id=uint160(currency)

   incidentalClaim += amount
     for every other incoming claim
   ```

   A verified whole-balance collector burn resets both components to zero. Any other outgoing claim
   is a hard stop. Require:

   ```text
   protocolClaim + incidentalClaim
     == PoolManager.balanceOf(Collector, uint160(currency))
   ```

3. Pin a fresh simulation block number and hash, extend the replay through that block, and query every
   claim balance. Include an ID only when:

   ```text
   0 < protocolClaim == PoolManager.balanceOf(Collector, id) <= type(int128).max
   incidentalClaim == 0
   ```

   If incidental value is present, omit that ID and abort its protocol-only collection. A deliberate
   whole-balance sweep to a quarantine destination is a separate accounting/governance action.

4. Construct `packed_assets` as the exact concatenation of unique 20-byte currency addresses. It is
   **not** ABI `address[]`. Require a nonempty length divisible by 20 and exact equality with the
   reviewed ID set. Include native ID `0` only deliberately and use a native-capable recipient.

5. Call:

   ```solidity
   ControllerV1.collect_unlock_swap_fees(treasury, packed_assets)
   ```

   Its selector is `0x33830e48`. The caller may be `ControllerV1.owner()` or `fastOwner()`. The call
   has no amount argument and drains each ID's **entire live balance**, including value received after
   the simulation block. Default-deny zero, Angstrom, Controller, Collector, PoolManager, and token
   contracts as recipients.

6. Simulate the exact caller, target, recipient, and bytes against the freshest numbered block. This
   preflight cannot freeze the inclusion-state balance. Prefer a dedicated receipt/quarantine account
   so a last-moment incidental claim cannot contaminate the main treasury. After execution, extend the
   replay to each exact burn prestate, including earlier calls in the same block; require one matching
   whole-balance ERC-6909 burn per ID and the matching underlying token transfer or native payment.
   Book only the execution-prestate `protocolClaim` as protocol revenue and quarantine any raced-in
   incidental component.

## 2. Calculate bundle-held protocol fees

Do not construct `distributeFees` calldata until every step passes for every asset.

1. Fix the chain, deployment transaction/trace, end block number and hash, deployed runtimes, compiler
   artifacts, and immutable values. Build the token universe from all bundle assets, deposits,
   withdrawals, pools including removed pools, controller pulls, collector proceeds sent to Angstrom,
   and every ERC-20 `Transfer` involving Angstrom. Carry exact CREATE-prestate balances as incidental.

2. Reconstruct gross saved commitments. In small overlapping block ranges, use two independent
   providers and decoders, prove gap-free coverage, and deduplicate by block hash/transaction/log
   index. For every Angstrom anonymous fee-summary log, locate the successful top-level or traced
   nested `execute(bytes)` input. Strictly decode every PADE section, reject asset/pair length
   remainders, require full payload consumption, and verify:

   ```text
   log.data == keccak256(concat_for_all_assets(assetAddress20 || saveUint128BE))
   savedGross[asset] = sum(verified save values)
   ```

3. Trace every successful `setController` and `pullFee(address,uint256)` call. Classify every pull by
   source and claimant from governance/distribution records; an unlabeled pull is a hard stop.

   ```text
   candidateOutstandingSaved
     = savedGross - pulledAgainstSaved
   ```

4. Apply one governance-approved ownership rule to every historical bundle. Reproduce the actual
   production builder version—including its saturating and floating-point semantics—and exact
   call-prestate pool configuration. Separately account for gas, referral fees, the selected protocol
   share, and builder residual. Treat residual as potentially LP-intended unless first-party evidence
   proves otherwise.

   ```text
   policyDerivedProtocolOutstanding
     = policyDerivedProtocolGross - protocolClaimsExecuted
   ```

5. Independently replay protected balances in transaction/call/order order. The equation below is for
   exact-transfer, non-rebasing tokens; otherwise use proved token-specific state and balance diffs:

   ```text
   U = all final Angstrom internal user balances
   L = all historical LP reward allocations - all actual LP payouts
   P = unspent protocol-origin collector proceeds sent to Angstrom
   X = independently provenance-tracked incidental surplus
   S = candidateOutstandingSaved

   ERC20.balanceOf(Angstrom) == U + L + S + P + X
   ```

   Preserve one row for every deposit, withdrawal, and internal-order balance delta, then check every
   discovered final internal-balance storage slot. Include allocations and payouts for removed pools,
   rounding dust, and zero-liquidity/unclaimable rewards. Require `X >= 0` and require its row-level
   provenance ledger to equal the arithmetic residual; never define `X` as a plug.

6. A proposed core withdrawal is eligible only if all of these hold in raw units:

   ```text
   savedGross >= pulledAgainstSaved
   lpAllocatedGross >= lpPayoutTotal
   0 <= policyDerivedProtocolOutstanding <= candidateOutstandingSaved
   0 <= otherActiveReservations
   0 <= proposedWithdrawal
   otherActiveReservations + proposedWithdrawal
     <= policyDerivedProtocolOutstanding
   ERC20.balanceOf(Angstrom) == U + L + S + P + X
   the same equality passes after reducing balance, S, and protocol outstanding by proposedWithdrawal
   ```

   `otherActiveReservations` excludes the candidate operation being evaluated. Therefore
   `maximumNewWithdrawal = policyDerivedProtocolOutstanding - otherActiveReservations` only after
   every gate passes; otherwise it is zero. The selected withdrawal may be smaller.

7. Generate `ControllerV1.distributeFees(Asset[])` (selector `0x182e6f39`) with a reviewed ABI encoder;
   `Asset = (address,uint256,Distribution[])` and `Distribution = (address,uint256)`. Only
   `ControllerV1.owner()` may call it; the current owner is the Timelock. Never call Angstrom's
   `pullFee` directly. Require unique supported assets, approved nonzero recipients, and
   `sum(distributions.amount) == total` for each asset. Reconstruct reservations from Timelock
   schedule/cancel/execute history plus the reviewed ledger. Reserve each scheduled amount/source
   range immediately, prevent overlapping proposals, and rerun every gate before readiness. Cancel
   on any change; execution is open once the Timelock operation is ready.

8. After execution, trace the exact `pullFee` and recipient transfers, remove only the executed
   reservation, reduce Angstrom balance, `candidateOutstandingSaved`, and
   `policyDerivedProtocolOutstanding` by the identical executed amount, and rerun the conservation
   equation. Any mismatch is an incident, not additional fee revenue.

## Hard stops

Collect nothing from the affected path if any runtime, immutable, role, source, token behavior,
builder version, referral split, trace, log range, PADE decode, commitment hash, prior-claim label,
internal balance, LP reserve, incidental provenance, reservation, calldata, recipient, or simulation
check is missing or mismatched. Preserve the raw RPC data, row-level ledgers, code artifacts, end-block
hash, generated calldata, simulation result, and execution receipt for independent review.
