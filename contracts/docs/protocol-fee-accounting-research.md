# Angstrom protocol-fee accounting: proofs, blockers, and collection paths

Research date: 2026-09-03. This note is read-only analysis of repository commit
`3690f9198321983f3700c5e417827335461e3651` plus a fixed, finalized Ethereum mainnet
snapshot. The verified deployed Angstrom source is semantically identical to the accounting paths
cited from that checkout; formatting and comments differ in a few files. This is **not** a completed
withdrawal authorization or an authoritative governance runbook. Before constructing a transaction,
re-run every chain calculation to a new finalized block, identify the production node/bundle-builder
version(s), complete the stated liability and ownership gates, and independently review the generated
calldata.

## Bottom line

There are two mechanically separate accounting and collection paths. They must not be combined:

1. **Uncollected unlocked-swap protocol fees** (`protocolUnlockedFee`) do **not** sit in Angstrom's
   ERC-20 or ETH balance. `afterSwap` mints Uniswap v4 ERC-6909 claims to a separate immutable
   `UnlockSwapFeeCollector`. The collection path burns the collector's whole claim balance and takes
   the underlying asset directly from the PoolManager to the recipient
   ([`UnlockHook.sol`](../src/modules/UnlockHook.sol#L70-L108),
   [`UnlockSwapFeeCollector.sol`](../src/modules/UnlockSwapFeeCollector.sol#L21-L41)). Its raw
   `PoolManager.balanceOf(collector, uint160(asset))` is the collector's exact claim balance at a
   specified block, but ERC-6909 claims are transferable. Classify only verified zero-to-collector
   mints whose event `caller` is Angstrom as protocol-originated; quarantine any other credit. A
   prior collection can deliberately send ERC-20 proceeds back to Angstrom because the recipient is
   arbitrary; account for such proceeds as a separate, trace-proven unlocked-fee credit. The
   collection call has no amount argument and withdraws the **live whole balance** for each supplied
   currency ID, so a historical snapshot is not a transaction amount.
2. **Bundle/book fees** are commingled with user balances, LP-designated value, and incidental value
   in Angstrom's ordinary ERC-20 balances. Each bundle asset's `save` is an aggregate accounting
   commitment: the contract documentation says it covers gas, exchange, and referral fees, while LP
   allocations are encoded separately in pool updates
   ([`bundle-building.md`](bundle-building.md#save)). The core does not store a running fee balance
   and does not tag any saved unit as GAS versus PROTOCOL.

For the bundle-held path, the core provides the following commitment ledger:

```text
candidateOutstandingSaved[asset]
  = sum(asset.save in every successful bundle through endBlock)
  - sum(prior pullFee amounts classified as claims against saved bundle fees)
```

During a successful bundle, `save` is subtracted from the current transaction's transient bundle
delta, and execution reverts unless that delta is exactly zero
([`Settlement.sol`](../src/modules/Settlement.sol#L64-L91),
[`DeltaTracker.sol`](../src/types/DeltaTracker.sol#L13-L29)). Under the documented standard-token
assumptions and a complete history, that proves a gross fee commitment was created by the bundle. It
does **not**, on its own, prove that the same units remain unspent, identify their owner, or authorize
a withdrawal. `pullFee` has no fee-accounting limit, source tag, event, or persistent fee ledger; up
to the token balance and the token's own transfer rules, it can physically transfer Angstrom-held
value when called by the controller. The design explicitly relies on the controller to verify
unclaimed fee-summary commitments
([`TopLevelAuth.sol`](../src/modules/TopLevelAuth.sol#L178-L183),
[`overview.md`](overview.md#L51-L54)). A missed historical pull or controller rotation, a token that
violates the transfer assumptions, an unexplained source/outflow, or a liability-reconciliation
shortfall is a hard stop. If unlocked-fee proceeds or another classified surplus was sent to
Angstrom, every historical `pullFee` additionally needs first-party source-allocation evidence;
blindly subtracting every pull from `save` would then be a conservative lower bound, not exact bucket
accounting.

The chain data does **not** define ownership of the aggregate `save` bucket. The anonymous
fee-summary log is only a hash commitment, so the values must first be recovered from successful
call input. Even after that, `save` combines gas, book-fee remainder, referrals, and builder residual;
previous `pullFee` calls carry no GAS/PROTOCOL label; and a user order's `extra_fee_asset0`
deliberately combines gas and referral fees ([`overview.md`](overview.md#L195-L206)). Exact
protocol-only accounting consequently needs a governance-approved classification rule plus
sufficient first-party records for historical claims, builder versions, and any nonzero referral
IDs. **No such rule or complete replay is established in this note. Therefore this note establishes
no currently authorized bundle-held protocol withdrawal amount.**

Current safety status:

| Accounting/collection path | What is proved at the pinned snapshot | What is not proved | Collection status |
|---|---|---|---|
| PoolManager claims owned by `FEE_COLLECTOR` | Fixed-block balances and protocol origin of the three enumerated ERC-20 claim balances | All-assets completeness, the future live whole balances, and the intended treasury recipient | Eligible only after the fresh provenance, size, recipient, calldata, role, and simulation gates below |
| ERC-20s held by Angstrom | Exact gross `save` commitments and no detected prior pull for the configured tokens | Protocol-only ownership and the full internal-user/LP liability reconciliation | **Do not collect through this path yet** |

Neither row authorizes a transaction. In particular, the collector call cannot encode the snapshot
amounts and no treasury recipient or live execution-state check was supplied to this research.

## Verified mainnet snapshot

This snapshot is for the current Angstrom deployment
`0x0000000aa232009084Bd71A5797d089AA4Edfad4`, created at block `22,971,782` in
[transaction `0x35cd…a12c`](https://eth.blockscout.com/tx/0x35cd06f2d4c1455ea7e1796355632b30a8ac9cf78ba100998c1ef837b078a12c).
It ends at finalized block `25,898,173`, hash
`0xaaef6de5a610c59d3f86bb0b32f9561cb30f44cc982d5756a004e98129868214`
(`2026-09-03T17:07:35Z`). The 23,569-byte runtime's Keccak-256 is
`0x528182e66a441545ea3ba1dede4e6063f23acd4ada4036c24b9fa2f254bc4ed1`.

Two independently executed RPC scans, backed by three providers and using 5,000-block requests,
returned byte-identical ordered sets of `871,535` unique anonymous fee-summary logs. Every
corresponding transaction was a direct, successful `execute(bytes)` call. Decoding 578,347,324 bytes
of calldata produced no trailing asset-section bytes, and every recomputed fee-summary hash matched
its receipt log. This establishes the gross-`save` totals only; a canonical, complete decode of all
nested sections was not used to establish the order, reward, or liability ledgers. These scan
results must be reproduced into a versioned machine-readable artifact before they are used to
approve a transaction; the totals below are research outputs, not a signed accounting report.

The deployment constructor input and end storage both identify
`0x1746484EA5e11C75e009252c102C8C33e0315fD4` as controller. The verified deployed `ControllerV1`
runtime permits its transition away only through `acceptNewController`, which emits
`NewControllerAccepted` before calling Angstrom; no `NewControllerSet` or `NewControllerAccepted`
event occurred through the snapshot. This proves no first rotation away only because the initial
controller identity and that controller's runtime were independently verified: Angstrom's own
`setController` emits no event, and a replacement controller could rotate back without using this
event. No USDC, WETH, or USDT `Transfer` from Angstrom to the verified initial controller occurred in
the interval. For these three standard deployed tokens, any nonzero successful `pullFee` while that
controller was active would create that transfer. Subject to those explicitly verified source,
deployment, log-completeness, and token assumptions, `pulledTotal` is zero through the snapshot. A
generalized calculator must still trace `setController` and `pullFee` calls rather than relying on
current endpoints or one recipient filter.

All amounts below were calculated as raw integers; decimals are display formatting only. This table
is deliberately limited to USDC, WETH, USDT, and native currency because those were the assets found
in the decoded bundles/current configured pools and the balances queried in this research. It is
not proof that no other token has ever entered Angstrom or the collector; the all-assets discovery
gate below is mandatory before claiming a complete contract-value reconciliation.

| Asset | Angstrom ERC-20 balance | Gross `save` commitment before liability replay | Arithmetic balance minus gross `save` (not liability-reconciled) | Protocol-origin collector claim at snapshot |
|---|---:|---:|---:|---:|
| USDC | 209,346.979059 | **36,411.377866** | 172,935.601193 | **28,418.032618** |
| WETH | 97.928275832040692424 | **4.859622321561737129** | 93.068653510478955295 | **23.203279237417803524** |
| USDT | 131,670.815475 | **0** | 131,670.815475 | **21,740.510974** |
| Native ETH | 0 | 0 / not a valid bundle asset | 0 | 0 |

Raw gross `save` totals are USDC `36,411,377,866`, WETH
`4,859,622,321,561,737,129`, and USDT `0`. These are exact collective gas/book/referral fee
commitments recovered through the snapshot, not a withdrawal ceiling and not protocol-only revenue.
The displayed subtraction does not enumerate user or LP liabilities. Calling the entire column
"protocol" would require an explicit policy assigning gas, referrals, and builder residual to the
protocol, historical support for that policy, and a passing liability replay.

The unlocked-claim column is separately protocol-origin-proven at the snapshot. Replaying PoolManager ERC-6909 `Transfer`
events found 91,974 USDC mints, 155,958 WETH mints, and 72,111 USDT mints where `caller = Angstrom`,
`from = 0`, and `to = FEE_COLLECTOR`; it found no other incoming credits and no outgoing transfers
or burns. Each event sum exactly equaled `PoolManager.balanceOf` at the end block. The collector was
`0x59a82241ce490fB77370e3f614de04Aa188e13d2`; its address independently equals Angstrom's nonce-1
CREATE child, and chain explorer creation metadata identifies Angstrom as its creator. Given the
no-outflow replay, equality between post-creation mint sums and ending balances also forces the
opening claim balance to zero for these three IDs; it says nothing about an unenumerated ID.

The snapshot's actionable distinction is therefore:

- the unlocked-claim amounts are an exact historical snapshot of segregated, protocol-originated
  claims; the live whole balances are collected through `collect_unlock_swap_fees`; and
- the gross `save` amounts are evidence of bundle fee commitments, but they are **not** an approved
  withdrawal boundary or a chain-provable protocol-only amount. Do not pass any amount derived from
  this table to `distributeFees` until the policy and liability gates below are complete.

## Exact deployed collection entry points

At the pinned block, the control plane was:

| Role | Address / value |
|---|---|
| Angstrom | `0x0000000aa232009084Bd71A5797d089AA4Edfad4` |
| Angstrom controller / `ControllerV1` | `0x1746484EA5e11C75e009252c102C8C33e0315fD4` |
| `ControllerV1.owner()` | `0x60D41d9708BBEfd29000d1486C6406Ef23526c01` (`TimelockController`) |
| `ControllerV1.fastOwner()` | `0xD31C82069da3013fdB16B731AD19076Af9b93105` (Safe proxy) |
| `ControllerV1.setController()` | `address(0)`; no pending controller candidate |
| Timelock `getMinDelay()` | `432000` seconds (5 days) |
| Timelock proposer and canceller | `fastOwner` Safe held both roles |
| Timelock executor | Open: `address(0)` held `EXECUTOR_ROLE` |
| Legacy `TIMELOCK_ADMIN_ROLE` hash | Timelock did not hold this role; the deployed OpenZeppelin version uses `DEFAULT_ADMIN_ROLE` |
| Uniswap v4 PoolManager | `0x000000000004444c5dc75cB358380D2e3dE08A90` |
| Immutable unlocked-fee collector | `0x59a82241ce490fB77370e3f614de04Aa188e13d2` |

These are snapshot facts, not permanent permissions. Read the delay, every role, owner, fast owner,
pending controller, runtime hashes, and immutable addresses again before scheduling or executing.
In particular, do not use the current deployment script's two-week constant as live state: the
pinned contract returned five days. The local Angstrom, ControllerV1, and collector runtime
artifacts—respectively 23,569, 10,552, and 2,111 bytes—match the deployed runtimes after masking only
the compiler-declared immutable ranges; there were zero other byte mismatches. The deployed
immutable values resolve to the addresses above. The Angstrom deployed runtime's Keccak-256 is
`0x528182e66a441545ea3ba1dede4e6063f23acd4ada4036c24b9fa2f254bc4ed1`.
Blockscout publishes verified source for
[Angstrom](https://eth.blockscout.com/address/0x0000000aa232009084Bd71A5797d089AA4Edfad4?tab=contract),
[ControllerV1](https://eth.blockscout.com/address/0x1746484EA5e11C75e009252c102C8C33e0315fD4?tab=contract),
the
[collector](https://eth.blockscout.com/address/0x59a82241ce490fB77370e3f614de04Aa188e13d2?tab=contract),
and the
[TimelockController](https://eth.blockscout.com/address/0x60D41d9708BBEfd29000d1486C6406Ef23526c01?tab=contract).
Explorer verification is supporting evidence, not a substitute for matching runtime bytecode. The
runtime comparison used locally generated compiler artifacts and compiler-declared immutable
reference ranges; before an operational decision, preserve the exact artifacts, compiler settings,
immutable-range manifest, deployed bytecode, and comparison program as reviewable outputs.

### Bundle-held fees: `ControllerV1.distributeFees`

The deployed call chain is:

```text
TimelockController (ControllerV1 owner)
  -> ControllerV1.distributeFees(Asset[])
  -> Angstrom.pullFee(asset, total)
  -> ERC-20 transfers from ControllerV1 to the listed recipients
```

`distributeFees` has selector `0x182e6f39`; the nested `pullFee` selector is `0xd9e17f98`.
Only `ControllerV1.owner()` may call `distributeFees`. The `fastOwner` Safe is **not** authorized for
this function. Calling Angstrom's `pullFee` directly from the Timelock, Safe, or an EOA also fails,
because Angstrom accepts only its current controller contract
([`ControllerV1.sol`](../src/periphery/ControllerV1.sol#L224-L239),
[`TopLevelAuth.sol`](../src/modules/TopLevelAuth.sol#L178-L183)).

Each ABI-encoded `Asset` is `(address addr, uint256 total, Distribution[] dists)`, and each
`Distribution` is `(address to, uint256 amount)`. The function first pulls `total` into ControllerV1,
then transfers each distribution, and reverts atomically unless `sum(dists.amount) == total`.
Operational generation must additionally reject duplicate assets, unsupported/nonstandard tokens,
zero recipients, zero/duplicate distributions, ControllerV1 itself as a recipient, Angstrom as a
recipient unless deliberate commingling is documented, and a token contract, PoolManager, or the
collector as recipient unless that exceptional destination has been explicitly reviewed. Require
each raw-integer `total` to equal its distributions' independently recomputed raw-integer sum and
reject overlapping fee-claim block ranges. Generate this dynamic calldata with a reviewed ABI
encoder rather than by hand.

At the pinned state, the Safe can schedule and cancel this Timelock operation and anyone can execute
it once ready. The encoded `total` is fixed at scheduling, while deposits, withdrawals, bundles, LP
claims, and other governance operations continue during the delay. Reserve every scheduled amount
and source range immediately in the off-chain ledger; forbid a second proposal from spending that
reservation; and rerun the complete code, ownership, source, liability, token-behavior, and calldata
gates before the operation becomes ready. Cancel it before readiness if any gate changes. Once ready,
an arbitrary caller can race to execute it. `distributeFees` checks distribution arithmetic and
executes the token transfers, but has no on-chain test that `total` is fee-owned or that user/LP
liabilities remain covered.

**Current status:** this note has not calculated an approved protocol-only `total` for any
bundle-held asset and has not completed the liability replay. It supports no nonzero
`distributeFees` payload and does not recommend scheduling even a no-op; do not derive a
bundle-fee distribution from the snapshot table.

### Unlocked fees: `ControllerV1.collect_unlock_swap_fees`

The deployed call chain is:

```text
ControllerV1 owner OR fastOwner
  -> ControllerV1.collect_unlock_swap_fees(to, packed_assets)
  -> Angstrom.collect_unlock_swap_fees(...)
  -> immutable collector.withdraw_to(...)
  -> PoolManager burns each whole ERC-6909 claim and sends underlying to `to`
```

The selector is `0x33830e48`. Despite its name, `ControllerV1._checkFastOwner()` accepts **either**
`fastOwner()` or `owner()` ([`ControllerV1.sol`](../src/periphery/ControllerV1.sol#L119-L123),
[`ControllerV1.sol`](../src/periphery/ControllerV1.sol#L299-L303)). Read-only calls at the pinned
state confirmed both identities pass this authorization branch and an unrelated address reverts.

`packed_assets` is not an ABI-encoded `address[]`. It is the raw concatenation of 20-byte currency
addresses. For the three configured ERC-20s in ascending address order it is:

```text
0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2dac17f958d2ee523a2206206994597c13d831ec7
```

The deployed collector does not validate this byte string: it processes `floor(length / 20)` and
silently ignores trailing bytes. The transaction-building layer must require `length > 0`,
`length % 20 == 0`, canonical 20-byte currency IDs in a reviewed order, exact equality with the
expected ID set, and no duplicates. Duplicate IDs do not double-collect—the first iteration drains
the claim and the second sees zero—but they are an encoding error. By default reject recipient
`address(0)`, the collector, PoolManager, ControllerV1, and Angstrom. The first three can burn or
strand value; the latter two commingle or strand it. Permit an exceptional destination only with a
separate, explicit accounting review. For native ID `0`, include it only deliberately and require
the final treasury recipient to receive native ETH successfully.

There is **no amount parameter**. For every supplied ID, the collector reads and burns its entire
balance at execution, including fees accrued after a snapshot and any third-party claim transfer.
Consequently the bold snapshot values are not encoded into this call and cannot be guaranteed as the
eventual receipt amount. Require every live claim balance to be no larger than
`type(int128).max`: both PoolManager `burn` and `take` cast the amount to `int128`, so a larger whole
balance cannot be partially collected by this interface
([`PoolManager.sol`](../lib/v4-periphery/lib/v4-core/src/PoolManager.sol#L290-L295),
[`PoolManager.sol`](../lib/v4-periphery/lib/v4-core/src/PoolManager.sol#L331-L335)). Immediately
before execution, refresh the provenance ledger and simulate the exact target, caller, recipient,
and byte string against the same intended state/prestate. After execution, record the actual
pre-burn balances and require matching ERC-6909 burns
`Transfer(caller=collector, from=collector, to=0, id, amount)` plus matching underlying transfers to
the intended recipient. A new or unclassified incoming claim before inclusion changes accounting;
quarantine that excess rather than silently reporting it as protocol revenue.

## What the core commits to

The deployed PADE asset encoding is:

```text
20-byte asset address || 16-byte save || 16-byte take || 16-byte settle
```

The array is prefixed by a three-byte byte length, and asset addresses must be strictly increasing
([`Asset.sol`](../src/types/Asset.sol#L20-L50),
[`CalldataReader.sol`](../src/types/CalldataReader.sol#L153-L163)). At the end of every successful,
nonempty bundle, Angstrom builds this preimage:

```text
concat_for_each_asset(20-byte asset address || 16-byte save)
```

and emits one anonymous, zero-topic log whose 32-byte data is the Keccak-256 of that preimage
([`Settlement.sol`](../src/modules/Settlement.sol#L67-L103),
[`Bundle.sol`](../test/_reference/Bundle.sol#L42-L49)). The tests explicitly check the same preimage
and then demonstrate that the controller can pull the committed amounts
([`Settlement.t.sol`](../test/modules/Settlement.t.sol#L285-L340)). Empty `execute("")` calls return
before unlocking or emitting the summary ([`Angstrom.sol`](../src/Angstrom.sol#L52-L55)).

Therefore:

- **Logs alone are insufficient.** A Keccak digest does not expose `(asset, save)` values.
- **Logs plus call input are sufficient only for the gross `save` accrual ledger.** Locate the successful
  `execute(bytes)` call in the transaction (use its top-level input when direct, or its call-trace
  input when nested), extract its `assets`, recompute the preimage, and require the digest to equal
  the receipt log before accruing it. This proves neither fee ownership nor present solvency.
  Historical `pullFee` calls, token behavior, source provenance, and the liability replay are
  additionally required before treating any result as withdrawable.
- The repository already has the matching decoder: `AngstromBundle` derives `PadeDecode`, and
  `Asset` exposes `addr`, `save`, `take`, and `settle`
  ([`angstrom/mod.rs`](../../crates/types/primitives/src/contract_payloads/angstrom/mod.rs#L30-L39),
  [`contract_payloads/mod.rs`](../../crates/types/primitives/src/contract_payloads/mod.rs#L13-L28)).

Ethereum's standard JSON-RPC exposes receipt logs, transaction input, and historical log filtering;
use an end block identified by hash (or a finalized block) and preserve transaction/block hashes in
the audit artifact ([ethereum.org JSON-RPC documentation](https://ethereum.org/developers/docs/apis/json-rpc/)).

## Every relevant value-changing path

| Path | Angstrom asset balance | Accounting effect | On-chain evidence |
|---|---:|---|---|
| `deposit(asset, amount)` / `deposit(asset, to, amount)` | `+amount` ERC-20 | Credits `_balances[asset][owner]` by the same amount | No Angstrom event; calldata plus token transfer log/state transition ([`Settlement.sol`](../src/modules/Settlement.sol#L22-L36)) |
| `withdraw(asset, ...)` | Normally `-amount`; zero balance change when `to == Angstrom` | Debits caller's internal balance; a self-transfer destination releases the liability into incidental surplus instead of moving tokens | No Angstrom event; calldata plus token transfer log/state transition ([`Settlement.sol`](../src/modules/Settlement.sol#L38-L46)) |
| Bundle `_take` | `+take` from PoolManager | Adds to transient bundle delta | Bundle calldata and call trace ([`Settlement.sol`](../src/modules/Settlement.sol#L48-L61)) |
| External order input/output | Normally transfer in or out; an output whose recipient is Angstrom is a token self-transfer | Adds/subtracts transient delta; no persistent user balance. A self-transfer output leaves additional incidental surplus and must not be mistaken for a fee | Bundle calldata, token logs, and trace ([`Settlement.sol`](../src/modules/Settlement.sol#L106-L128)) |
| Internal order input/output | No token transfer | Debits/credits persistent user balances while changing transient delta | Bundle calldata and storage replay; no event ([`Settlement.sol`](../src/modules/Settlement.sol#L106-L128)) |
| User composability hook | No direct core balance entry; called with zero ETH | Arbitrary external hook code can make nested deposits/withdrawals or send tokens to Angstrom; those effects retain their normal classification | Decode the signed hook and trace its call subtree ([`HookBuffer.sol`](../src/types/HookBuffer.sol#L82-L113)) |
| Pool reward allocation | No immediate transfer | Subtracts the full LP-designated `rewardTotal` from transient delta. Normally it updates reward growth, but a `currentOnly` entry with `amount > 0` and `expectedLiquidity == 0` returns the amount without updating growth, making it designated but unclaimable | `RewardsUpdate` in bundle calldata ([`PoolUpdates.sol`](../src/modules/PoolUpdates.sol#L171-L215), [`GrowthOutsideUpdater.sol`](../src/modules/GrowthOutsideUpdater.sol#L31-L58)) |
| Bundle `_saveAndSettle` | `-settle`; the current bundle's `save` net remains in Angstrom | Commits an aggregate bundle surplus under the standard-token assumptions and pays PoolManager settlement; it does not identify ownership or maintain a balance | Anonymous hash log plus bundle calldata; ERC-20 transfer for nonzero settle ([`Settlement.sol`](../src/modules/Settlement.sol#L64-L103)) |
| LP reward claim in `beforeRemoveLiquidity` | `-rewards` to PoolManager when `rewards > 0` | Pays the computed pending Angstrom reward and advances the checkpoint only on a nonzero payout | No Angstrom event; call trace/storage transition and token transfer ([`PoolUpdates.sol`](../src/modules/PoolUpdates.sol#L123-L154)) |
| `pullFee(asset, amount)` | `-amount` to current controller | Intended to reduce saved fee surplus, but can physically consume any Angstrom-held balance and has no source-bucket ledger | Call trace and token transfer; no Angstrom event ([`TopLevelAuth.sol`](../src/modules/TopLevelAuth.sol#L178-L183)) |
| Unlocked swap `afterSwap` | No Angstrom balance change | Mints a protocol-fee ERC-6909 claim to `FEE_COLLECTOR` | PoolManager ERC-6909 `Transfer` mint log and collector balance ([`UnlockHook.sol`](../src/modules/UnlockHook.sol#L79-L100), [`ERC6909.sol`](../lib/v4-periphery/lib/v4-core/src/ERC6909.sol#L79-L89)) |
| `collect_unlock_swap_fees` | Normally none; `+amount` ERC-20 if `to == Angstrom` | Burns the collector's **entire** claim balance for every packed asset, then PoolManager pays the arbitrary recipient | ERC-6909 burn plus underlying transfer/native value movement ([`TopLevelAuth.sol`](../src/modules/TopLevelAuth.sol#L71-L74), [`UnlockSwapFeeCollector.sol`](../src/modules/UnlockSwapFeeCollector.sol#L21-L41)) |
| Direct token transfer, token mint/burn/rebase, forced ETH, or predeployment address funding | Arbitrary | No Angstrom internal credit, LP allocation, or fee commitment | Opening balance plus token/native state and possibly token logs; it is incidental unless independently documented |

`beforeAddLiquidity` changes only reward checkpoints; it transfers no asset
([`PoolUpdates.sol`](../src/modules/PoolUpdates.sol#L49-L120)). For a conforming permit token, permit
composition changes allowance/nonce state but intentionally transfers no value; nonstandard token
effects still require trace and token-specific review
([`PermitSubmitterHook.sol`](../src/modules/PermitSubmitterHook.sol#L20-L74)). No other core source
file calls `safeTransfer`, `safeTransferFrom`, PoolManager `take`, `mint`, or `burn`.

### Why ERC-20 Transfer logs are not enough

The same transfer direction can have several meanings. In particular, Angstrom-to-PoolManager can
be normal bundle settlement or an LP reward claim, and Angstrom-to-an-address can be a user output,
an internal-balance withdrawal, or a controller fee pull. Internal orders change liabilities with no
token transfer at all. Deposits, withdrawals, and position callbacks can themselves be nested inside
another transaction. Exact classification therefore requires decoded calldata and call traces or
deterministic state replay. Geth's `callTracer` exposes nested calls, while its
`prestateTracer` diff mode can expose touched state; obtaining old traces may require an archive/reexec
capable node ([Geth built-in tracer documentation](https://geth.ethereum.org/docs/developers/evm-tracing/built-in-tracers),
[Geth debug namespace](https://geth.ethereum.org/docs/interacting-with-geth/rpc/ns-debug)).

### Conditional conservation proof

For one standard ERC-20 and a complete, execution-ordered replay, define `B` as Angstrom's token
balance, `U` as the sum of its internal user balances, `L` as allocated-but-unpaid LP value, `S` as
outstanding aggregate `save` commitments, `P` as unlocked-fee proceeds trace-proven to remain at
Angstrom, and `X` as independently provenance-tracked incidental value. A successful bundle's
transient check proves:

```text
externalIn - externalOut + internalIn - internalOut
  + take - rewardTotal - save - settle = 0
```

For non-self, exact-value transfers, `delta(B) = externalIn - externalOut + take - settle`,
`delta(U) = -internalIn + internalOut`, `delta(L) = rewardTotal`, and `delta(S) = save`.
Therefore `delta(B) = delta(U + L + S)`. Deposits and ordinary withdrawals change `B` and `U`
equally; LP payouts change `B` and `L` equally; and a source-classified `pullFee` changes `B` and the
selected one of `S`, `P`, or `X` equally. A collector payout to Angstrom increases `P` by its
protocol-proven portion and `X` by any incidental portion. Direct transfers and predeployment
funding increase `X`.

Two valid self-transfer cases need explicit adjustments. `withdraw(..., to = Angstrom)` decreases
`U` while leaving `B` unchanged, so it increases `X` by the same amount. An external order output to
Angstrom reduces the transient delta without reducing `B`, so it likewise increases `X`. Any
self-transfer input or other anomalous balance behavior must use the observed balance change and
book `delta(X) = observed delta(B) - nominal standard-path delta(B)` for that transfer; nominal
calldata amounts are insufficient.
Nested hook effects are replayed as their actual individual calls, not netted away.

Consequently the desired end-state equality is:

```text
B = U + L + S + P + X
```

This is a proof obligation, not a fact established by the gross-`save` scan. It holds only if opening
balances are known; every value-changing call and nested call is replayed in EVM order; every source
and outflow is classified exactly once; all internal-balance owners and LP allocations/payouts are
covered; the controller and collector histories are complete; and the token moves the exact stated
amount without transfer fees, rebases, token-side callbacks, silent burns/mints, or other
nonstandard behavior. For a token that violates those assumptions, replace the nominal equations
with token-specific balance/state diffs and prove its semantics. Any unexplained difference is a
hard stop and must not be assigned to fees.

## Exact extraction procedure for bundle-held fees

Choose and persist `(chainId, Angstrom address, deployment block, deployment transaction and trace,
end block number, end block hash, runtime bytecode hash, verified source/compiler settings)`. Do not
mix deployments or calculate to a moving `latest` head. The mainnet script obtains a deterministic
address from `VANITY_MARKET` and asks that contract to deploy the initcode
([`BaseScript.sol`](../script/BaseScript.sol#L7-L20),
[`Angstrom.s.sol`](../script/Angstrom.s.sol#L53-L58)); do not substitute the test helper's deployment
method when reconstructing mainnet. Reconstruct every tracked token's and ETH's balance at the exact
CREATE-frame prestate in the deployment transaction: a deterministic address can be funded before
its code exists, including earlier in the same block or transaction, so the preceding block's state
alone is insufficient. Carry that opening balance as incidental unless provenance proves otherwise.

First build a token universe; the contract has no enumerable list of ERC-20 balances. Union the
addresses from every decoded bundle asset list, every successful `deposit`, `withdraw`, and
`pullFee` call (including nested calls and every historical controller), every configured and
removed pool, every PoolManager claim ID ever transferred to or from the collector, and every
standard ERC-20 `Transfer` involving Angstrom found by an address-topic indexer or a full receipt
scan. Include native currency separately and inspect the deployment prestate and value traces.
Unknown, unsolicited, or airdropped tokens are incidental by default—never protocol revenue. A
malicious/nonstandard token can change `balanceOf(Angstrom)` without a standard `Transfer` event, so
no generic log-only procedure can prove a universe of every possible token contract; restrict any
withdrawal allowlist to tokens whose full history and semantics have been proved.

1. Fetch every log emitted by the Angstrom address from deployment through the fixed end block.
   Select zero-topic, 32-byte logs from the deployed version's fee-summary path. Fetch the containing
   successful transaction, locate the corresponding successful `execute(bytes)` call, and decode its
   PADE payload. If `execute` is nested, obtain the call input from a trace rather than assuming the
   top-level transaction input is the Angstrom call. If a transaction ever contains multiple
   successful `execute` frames/logs, associate them by trace and log execution order rather than by
   transaction hash alone.
2. For each decoded asset entry, recompute
   `keccak256(concat(address20, save16))` across the complete sorted list and require exact equality
   with the log data. Record `(blockHash, txHash, logIndex, asset, save)` and accrue
   `savedGross[asset] += save` using arbitrary-precision integers. Require the asset section byte
   length to be divisible by 68 and the pair section byte length to be divisible by 38: the deployed
   readers floor both counts and skip any remainder. For protocol classification and liability
   replay, require exact consumption of every nested PADE section and of the full payload; do not
   treat a successful on-chain call as proof that its encoding was canonical
   ([`Asset.sol`](../src/types/Asset.sol#L31-L50),
   [`Pair.sol`](../src/types/Pair.sol#L50-L66),
   [`Angstrom.sol`](../src/Angstrom.sol#L57-L74)).
3. Trace all successful calls into the Angstrom address and select the exact
   `pullFee(address,uint256)` selector. Record the caller/controller active at that point, amount,
   transaction, and matching balance movement. Accrue `pulledTotal[asset] += amount`. Do not infer
   pulls merely by looking for transfers to one controller address: controllers can change and the
   same recipient may receive unrelated transfers. Independently reconstruct every controller
   transition from successful `setController` calls and storage, including a rotation away and back.
   Classify each pull's source and claimant from its distribution/governance evidence, and require
   the classified amounts to sum to `pulledTotal`.
4. Compute `candidateOutstandingSaved = savedGross - pulledAgainstSaved`. If no other classified
   source was ever credited to Angstrom, a reviewed accounting policy may set
   `pulledAgainstSaved = pulledTotal`; that is not an automatic inference. Otherwise an unlabeled
   allocation is a hard stop. Require the result to be nonnegative and no larger than
   `ERC20(asset).balanceOf(Angstrom)` at the exact end block. This is a candidate collective
   commitment remainder, not yet a safe or protocol-owned withdrawal amount.
5. Run the independent liability reconciliation below. Any mismatch blocks a withdrawal; do not
   "fix" it by assigning unexplained balance to protocol fees.

For every `eth_getLogs` extraction, use small bounded inclusive ranges (the snapshot used 5,000
blocks), record exact first/last block hashes, cover boundaries with no gaps, deliberately overlap
and deduplicate by `(blockHash, transactionHash, logIndex)`, retain the raw responses, and compare at
least two independently implemented scans backed by independent providers. Fail closed on any
disagreement. During this research, one successful 100,000-block provider response silently omitted
logs; absence of an RPC error is not evidence of completeness.

The summary log is emitted only after `_saveAndSettle` has checked every asset and performed all
settlements, so a matching receipt commitment is materially stronger than parsing any attempted or
reverted transaction ([`Settlement.sol`](../src/modules/Settlement.sol#L75-L103)).

## Separating book protocol fees from gas/referral fees

This separation is a policy calculation above the core contract, not a field in contract storage.
The two definitions below are **illustrative candidates**, not findings that either policy was
adopted. Governance/accounting owners must select and evidence a rule before any core withdrawal.

### Definition A: saved residual

If the protocol's accounting policy defines "protocol" as every committed `save` unit that is not an
order's charged extra fee, compute per successful bundle and asset:

```text
chargedExtra[token0]
  = sum(ToBOrder.gas_used_asset_0)
  + sum(UserOrder.extra_fee_asset0)

candidateProtocolGrossA[asset] = savedGross[asset] - chargedExtra[asset]
```

The bundle payload exposes both actual charged fields
([`tob.rs`](../../crates/types/primitives/src/contract_payloads/angstrom/tob.rs#L34-L43),
[`order.rs`](../../crates/types/primitives/src/contract_payloads/angstrom/order.rs#L62-L75)). The
contract always charges the user-order extra amount in token0, regardless of trade direction
([`UserOrderBuffer.sol`](../src/types/UserOrderBuffer.sol#L186-L216)), and applies ToB gas similarly
([`Angstrom.sol`](../src/Angstrom.sol#L127-L164)).

This is exact only for the stated classification **and** the historical node convention that charged
extras were committed into `save`; the core does not enforce a link between an order's extra field
and the asset's save field. Check `chargedExtra[asset] <= savedGross[asset]` per bundle and verify the
builder version or first-party bundle record. Any violation is a hard stop, not a negative protocol
fee. `extra_fee_asset0` is **gas plus referral**, not a gas-only field, so nonzero `ref_id` orders need
the historical referral registry/node calculation if gas and referrals belong to different
claimants. The node chooses these actual fee fields and the core merely checks them against
user-signed maxima
([`overview.md`](overview.md#L73-L82),
[`UserOrderBuffer.sol`](../src/types/UserOrderBuffer.sol#L186-L193)). Both checked-in builder paths
shown in this checkout construct user orders with `ref_id: 0` and put the calculated/max-gas-path gas
amount into the extra field, but that is not an on-chain guarantee and must be verified over the
target history
([`user_orders.rs`](../../crates/types/src/traits/user_orders.rs#L75-L97),
[`user_orders.rs`](../../crates/types/src/traits/user_orders.rs#L101-L162)).

### Definition B: configured book-fee share

If "protocol" specifically means the non-LP share of `bundleFee`/`feeInE6`, reproduce the historical
bundle builder exactly for each pool in each successful bundle. Resolve the actual `feeInE6` from
the historical `PoolConfigStore` selected by that bundle's pair/store index; it is not encoded
directly in the payload ([`Pair.sol`](../src/types/Pair.sol#L89-L106),
[`PoolConfigStore.sol`](../src/libraries/PoolConfigStore.sol#L128-L151)). Use the exact call prestate,
not merely end-of-block state: a configuration update and bundle can occur in the same block. In the
current checkout the builder:

1. computes each filled user's token0 exchange fee with `get_quantities_at_price` and
   `saturating_add`s it into `total_user_fees`
   ([`bundles.rs`](../../crates/types/src/traits/bundles.rs#L310-L336),
   [`matching_math.rs`](../../crates/types/primitives/src/primitive/matching_math.rs#L43-L129));
2. computes `total_lp_user_donate = (total_user_fees as f64 * 0.75) as u128`; and
3. treats the remainder as `save_amount`
   ([`bundles.rs`](../../crates/types/src/traits/bundles.rs#L407-L424),
   [`angstrom/mod.rs`](../../crates/types/primitives/src/contract_payloads/angstrom/mod.rs#L25-L25)).

Do not replace the saturating operations or step 2 with idealized integer/rational arithmetic: the
checked-in implementation clamps its `u128` sum, converts that integer to `f64`, multiplies it, and
then casts to `u128`. A bit-exact historical reproduction must preserve the historical Rust
toolchain's conversion, floating-point, and cast semantics.
More importantly, this 75% split is node software policy, not enforced by Angstrom. The contract
only sees the final reward updates and `save`; trusted nodes supply prices, fills, and actual extra
fees ([`overview.md`](overview.md#L67-L82)). The split was introduced in repository commit
[`6fc9b4ff`](https://github.com/SorellaLabs/angstrom/commit/6fc9b4ff2d20bd0e550c631005f1ea7bf86f3510),
which predates the current mainnet deployment, but repository timing does not prove which binary
each production node ran. Applying the current formula to a differently-built bundle would not be
exact.

The builder also sweeps residual contract liquidity into `save` and describes it as usually scraps
and rounding errors ([`asset/state.rs`](../../crates/types/primitives/src/contract_payloads/asset/state.rs#L167-L187)).
That residual is not necessarily protocol dust. Donation construction uses saturating arithmetic and
can encode less LP reward than the nominal donation input, leaving intended-but-unallocated LP value
to be swept into `save`
([`pool_swap.rs`](../../crates/types/src/uni_structure/pool_swap.rs#L237-L349)). Consequently
`savedGross - chargedExtra` need not equal the configured book-fee remainder. Calculate all three
values and quarantine the residual as potentially LP-intended unless first-party policy and a
bit-exact bundle reconstruction prove otherwise; never silently call it protocol revenue.

Only the output of the governance-selected, historically evidenced rule is called
`policyDerivedProtocolGross` below. `candidateProtocolGrossA` is merely Definition A's candidate;
neither candidate becomes protocol-owned because its arithmetic succeeds.

### Prior claims are an additional ambiguity

`pullFee(asset, amount)` does not encode a claim type. `ControllerV1.distributeFees` gives a total and
recipients, but likewise contains no GAS/PROTOCOL tag
([`ControllerV1.sol`](../src/periphery/ControllerV1.sol#L224-L239)). The newer `EventEmitter` defines
manual `FeeClaimQueued` and `FeeClaimExecuted` events with `GAS`, `PROTOCOL`, or `BOTH`, but an admin
can emit them independently of the actual pull and `BOTH` does not state the split
([`EventEmitter.sol`](../src/periphery/EventEmitter.sol#L8-L29),
[`EventEmitter.sol`](../src/periphery/EventEmitter.sol#L45-L72)). Its originating PR explicitly
describes it as a periphery observability mechanism rather than enforcement
([PR #675](https://github.com/SorellaLabs/angstrom/pull/675)).

Thus:

```text
policyDerivedProtocolOutstanding
  = policyDerivedProtocolGross - protocolClaimsExecuted
```

is exact only after `policyDerivedProtocolGross` has itself passed the historical builder, ownership,
and classification gates **and** every prior collective `pullFee` amount can be assigned to a claim
type. For an unlabeled old pull or a `BOTH` event with no allocation, chain history provides only the
collective outstanding amount, not the protocol-only remainder. That is an information gap, not a
rounding problem; stop and obtain the governance proposal/calculation or other first-party
accounting record.

## Unlocked-swap protocol fees: exact state accounting

`TopLevelAuth` creates one immutable collector in the Angstrom constructor
([`TopLevelAuth.sol`](../src/modules/TopLevelAuth.sol#L42-L64)). Obtain its address from the deployment
trace, from an ERC-6909 mint log, or independently derive and verify the constructor's child CREATE
address. Do not guess it from a later recipient. Because the deterministic Angstrom address and its
nonce-1 child can be known before creation, query/replay the PoolManager claim balance at the
collector address immediately before its creation for every discovered ID. Carry any opening claim
as incidental; post-creation provenance alone does not make it protocol revenue.

For every supported currency `a`, query at the fixed end block:

```text
collectorClaimBalance[a]
  = PoolManager.balanceOf(FEE_COLLECTOR, uint160(a))
```

This is exact **at that block only**. The amount actually collected is the balance read inside the
later transaction, as described in the operational section above.

PoolManager's `mint` credits that ERC-6909 balance and emits a `Transfer` from zero; `burn` debits it
and emits a `Transfer` to zero ([`PoolManager.sol`](../lib/v4-periphery/lib/v4-core/src/PoolManager.sol#L320-L335),
[`ERC6909.sol`](../lib/v4-periphery/lib/v4-core/src/ERC6909.sol#L79-L89)). For an independent
current-balance check, start with the exact pre-creation opening balance and replay every
post-creation incoming and outgoing `Transfer`; require
`openingClaimBalance + postCreationIncoming - postCreationOutgoing == collectorClaimBalance`.
A post-deployment mint/burn sum alone is insufficient unless both the opening balance and ordinary
claim transfers are proved zero.

Do not equate that raw balance with protocol provenance without checking all PoolManager ERC-6909
`Transfer` logs. ERC-6909 exposes public `transfer` and `transferFrom`, so an unrelated holder can
send a claim to the collector without its consent ([`ERC6909.sol`](../lib/v4-periphery/lib/v4-core/src/ERC6909.sol#L25-L47)).
Its event includes `caller`, `from`, `to`, `id`, and `amount`
([`IERC6909Claims.sol`](../lib/v4-periphery/lib/v4-core/src/interfaces/external/IERC6909Claims.sol#L10-L15)).
Replay those logs in order and maintain two components:

```text
protocolClaim = 0
incidentalClaim = openingClaimBalance

protocolClaim += amount
  only for Transfer(caller = Angstrom, from = 0, to = FEE_COLLECTOR, id = uint160(a))

incidentalClaim += amount
  for every other Transfer(to = FEE_COLLECTOR, id = uint160(a))
```

For every burn from the collector, verify from the transaction trace that it is the collector's
withdrawal callback and that its amount equals the complete pre-burn claim balance; then both
components reset to zero. With the deployed runtime match, origin classification rests on the actual
zero-to-collector mint, its `caller = Angstrom`, and the fact that this is Angstrom's only such mint
path—not on a theoretical fee calculation. As an independent defense-in-depth check, recompute each
mint from that transaction's successful `afterSwap` inputs/delta and configured rate. Resolve that
rate at the exact call prestate, including transaction and call ordering: a pool configuration or
batch update earlier in the same block can change it. At the end block require
`protocolClaim + incidentalClaim == collectorClaimBalance`. Only `protocolClaim` is an exact
uncollected unlocked protocol fee at that fixed block after the runtime, provenance, opening-balance,
and balance-reconciliation checks pass. A formula discrepancy, unexpected outgoing transfer, or
unclassified mint is still a hard stop for the audit.

The collector has no partial amount parameter: for each exact 20-byte address in `packed_assets`, it
reads its live whole claim balance, burns all of it, and takes all underlying to `to`
([`UnlockSwapFeeCollector.sol`](../src/modules/UnlockSwapFeeCollector.sol#L21-L41)). This path cannot
touch Angstrom's LP rewards or user balances because those assets are not in the collector account,
but it does also drain any incidental ERC-6909 claims transferred into the collector for that ID.
If `to` is Angstrom itself, the ERC-20 transfer credits Angstrom without changing an internal user
balance, LP reward accumulator, or bundle `save`. Split that receipt into protocol and incidental
components using the pre-burn claim ledgers above and track subsequent classified outflows. Sending
native ETH to Angstrom through this path should revert because Angstrom has no payable receive or
fallback; verify the deployed runtime and trace rather than assuming it succeeded
([`Currency.sol`](../lib/v4-periphery/lib/v4-core/src/types/Currency.sol#L40-L53)).

The hook's exact fee formula and fee currency are in `afterSwap`: exact-input fees use
`abs(target) * rate / 1e6`; exact-output fees gross up with
`abs(target) * 1e6 / (1e6 - rate) - abs(target)`, under Solidity integer division, and the claim is
minted in the swap's unspecified currency ([`UnlockHook.sol`](../src/modules/UnlockHook.sol#L79-L100)).
The formula was corrected in
[`PR #560`](https://github.com/SorellaLabs/angstrom/pull/560), reinforcing the need to match the
deployed bytecode rather than assume the present source for old deployments.

## Independent liability reconciliation — mandatory for a core withdrawal

This reconciliation was **not completed for the snapshot above**. The table therefore cannot be
used to authorize `distributeFees`.

For each ERC-20 asset at the same end block, independently calculate:

```text
actualBalance
  = internalUserLiability
  + lpDesignatedReserve
  + candidateOutstandingSaved
  + unspentUnlockedProceedsAtAngstrom
  + incidentalSurplus
```

where:

- `internalUserLiability` is the sum of all `_balances[asset][owner]`. The mapping is not enumerable.
  Reconstruct its owner set and deltas from deposits, withdrawals, and every decoded internal order,
  including ECDSA recovery or resolution of the encoded ERC-1271 signer and every optional recipient.
  Preserve one row per balance delta in exact transaction/call/order execution order, with owner,
  asset, signed delta, cause, and before/after value; aggregate totals alone cannot catch an
  incorrectly assigned debit and credit that cancel.
  Reject any replayed underflow, and then full-check every discovered final slot through `extsload`
  or `AngstromInspector.balanceOf`
  ([`AngstromView.sol`](../src/periphery/AngstromView.sol#L62-L70),
  [`AngstromInspector.sol`](../src/periphery/AngstromInspector.sol#L42-L48)).
- `lpDesignatedReserve = sum(all reward amounts encoded in successful PoolUpdates) - sum(all LP
  reward payouts)`. Independently require `lpAllocatedGross >= lpPayoutTotal` and a nonnegative
  reserve; never hide a payout deficit in incidental surplus. Reserve the full ledger remainder, not
  merely the sum currently claimable by enumerated positions, and include every historically
  configured pool, including pools later disabled, removed, or omitted from the current controller
  array. Reward division creates rounding dust, and rewards sent to a tick with no active liquidity
  can be permanently unclaimable; a
  `currentOnly` reward with zero expected liquidity is another explicitly unallocated case. The
  invariant model tracks unclaimable value as
  `ghost_unclaimableRewards` and only expects approximate equality across positions
  ([`AngstromInvariants.t.sol`](../test/invariants/AngstromInvariants.t.sol#L103-L163),
  [`known-issues.md`](known-issues.md#bundle-building-footguns)).
- `unspentUnlockedProceedsAtAngstrom` is the protocol component of successful collector withdrawals
  whose recipient was Angstrom, less later outflows proven to consume that component. Because
  `pullFee` has no source-bucket tag, an ambiguous later pull must remain unclassified and blocks an
  exact answer.
- `incidentalSurplus` must have its own row-level provenance ledger. Direct transfers are not
  deposits and are not fee-summary commitments. Include predeployment address funding, self-transfer
  adjustments, unknown incoming tokens, and the incidental component of collector receipts, less
  trace-proven incidental outflows. Independently compute the arithmetic balance residual and require
  it to equal this ledger; do not define incidental surplus as whatever value makes the equation
  balance. Leave it untouched unless a separate provenance proof classifies it.

Required safety gates before signing any withdrawal:

```text
savedGross >= pulledAgainstSaved
lpAllocatedGross >= lpPayoutTotal
lpDesignatedReserve = lpAllocatedGross - lpPayoutTotal >= 0
0 <= proposedBundleWithdrawal
0 <= otherActiveScheduledReservations
otherActiveScheduledReservations + proposedBundleWithdrawal
  <= policyDerivedProtocolOutstanding
policyDerivedProtocolOutstanding <= candidateOutstandingSaved
actualBalance >= internalUserLiability + lpDesignatedReserve + candidateOutstandingSaved
                 + unspentUnlockedProceedsAtAngstrom
arithmeticResidual
  = actualBalance - internalUserLiability - lpDesignatedReserve
    - candidateOutstandingSaved - unspentUnlockedProceedsAtAngstrom
  = independentlyTrackedIncidentalSurplus >= 0
```

Here `policyDerivedProtocolOutstanding` exists only after applying a governance-approved ownership
rule and subtracting all protocol-classified prior claims. Gas, referral, residual, and any other
claimant's share remains protected. `otherActiveScheduledReservations` is the sum of nonexecuted,
noncancelled Timelock distributions against that same asset/source bucket, excluding the candidate
operation being evaluated. Reserve the candidate before scheduling it; after scheduling, require
the active-reservation ledger to include exactly
`otherActiveScheduledReservations + proposedBundleWithdrawal`, even though no on-chain balance has
moved. Also require all source/claim classifications to sum to `pulledTotal`, and reconcile
`savedGross`, `pulledTotal`, and every
GAS/PROTOCOL/referral/residual subledger to independently generated per-transaction CSV/JSON rows and
a second implementation. Use raw integer units for every accounting ledger; the only floating-point
operation is a bit-exact reproduction of a historically verified Definition B builder, never token
display or final accounting arithmetic. Immediately before execution, require the exact proposal to
remain present in the active-reservation ledger. Model execution as equal reductions to
`actualBalance`, `candidateOutstandingSaved`, and `policyDerivedProtocolOutstanding`, remove only
that executed proposal from the ledger, then re-run the equations and require every protected
balance to remain fully covered.

## ETH is a special case

The normal Angstrom bundle asset list cannot contain `address(0)`: validation begins with
`lastAddr = address(0)` and rejects any first address `<= lastAddr`
([`Asset.sol`](../src/types/Asset.sol#L43-L48)). Angstrom has no payable `receive`/`fallback`, and its
deposit, withdrawal, reward payout, and `pullFee` paths use ERC-20 `safeTransfer` rather than the
library's ETH-transfer functions ([`Settlement.sol`](../src/modules/Settlement.sol#L22-L46),
[`TopLevelAuth.sol`](../src/modules/TopLevelAuth.sol#L178-L183),
[`SafeTransferLib.sol`](../lib/solady/src/utils/SafeTransferLib.sol#L286-L303)). Therefore a native
ETH balance on the Angstrom address is not a normal bundle protocol-fee balance; treat it as forced
or otherwise incidental, and note that the current `pullFee(address(0), amount)` path is not an ETH
withdrawal path.

Native ETH **can** be an unlocked-swap fee held as PoolManager claim ID `0`. PoolManager's currency
transfer code treats address zero as native ETH
([`Currency.sol`](../lib/v4-periphery/lib/v4-core/src/types/Currency.sol#L40-L53)), so it belongs in
the segregated collector calculation above, not in Angstrom's ETH balance.

## Deliverable an exact calculator should produce

Before proposing a multisig transaction, generate and retain:

- deployment/runtime-code identification, fixed end-block hash, complete token universe, and opening
  balances;
- raw, range-indexed RPC responses plus the two-provider/two-scanner completeness comparison;
- one row per fee-summary log with decoded assets, saves, recomputed hash, and pass/fail;
- one row per `pullFee`, including nested call trace and claim classification evidence;
- one row per order extra fee and, if Definition B is used, per-order book-fee computation;
- one row per LP reward allocation and payout;
- one execution-ordered row for every internal-balance delta and every incidental-source delta, with
  independently checked final storage slots and token balances;
- one row per collector claim transfer/mint/burn, with provenance, exact-call-prestate rate
  recomputation, and final PoolManager balance check;
- per-asset totals for internal liability, LP reserve, saved collective fees, protocol-only fees,
  gas/referral fees, unlocked-swap collector claims, unspent unlocked proceeds at Angstrom, actual
  balance, and incidental surplus;
- explicit hard failures for unknown code version, missing trace/input, any residual not matched by
  the independent provenance ledger, negative incidental surplus, RPC disagreement/gap,
  noncanonical PADE sections, unknown token behavior, unlabeled prior claims, nonzero referral IDs
  without a split source, or any hash mismatch; and
- exact reviewed calldata, simulation state/block, recipient allowlist result, timelock role/delay
  reads, pending/scheduled source-range reservations, and post-execution receipt reconciliation.

Only the resulting protocol-only outstanding column should feed a protocol withdrawal. The unlocked
collector column must use `collect_unlock_swap_fees`; the bundle-held column uses the controller's
`pullFee`/`distributeFees` path. They begin on separate accounting/collection paths and use separate
authorization paths; only the collector claims are segregated. Bundle value is already commingled
in Angstrom, and a collector payout to Angstrom adds another source that must be preserved as a
separate provenance ledger. Every withdrawal statement here remains conditional on all ownership,
completeness, token-behavior, liability, governance, recipient, calldata, and simulation gates
passing; this snapshot authorizes no transaction by itself.
