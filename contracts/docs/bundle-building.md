# Bundle Building

This document describes how a node is expected to derive certain parameters for the
payload which is then structured and formatted via the [payload types](./payload-types.md) and [PADE
encoding format](./pade-encoding-format.md).

Note that a lot of these things cannot or are not enforced at the contract level and rely on the
[Economic security & sufficiently staked assumptions](./overview.md#assumptions).


## Asset list

The asset list of a given bundle must encompass any asset referenced in the bundle.

The `save`, `take` & `settle` fields must be set according to the orders contained within the
bundle.

### `save`

The `save` amount determines the total in gas, exchange & referral fees to be committed to for later
collection. Exchange fees are computed by the pair's `fee_in_e6`. Note that `save` only includes the
amount to be distributed to nodes, LP fees are attributed within the bundle via pool updates.

### `take`

The `take` amount is the total in liquid tokens to be drawn in from Uniswap at the start of the
bundle. This serves to:
1. Settle the total output from the swaps in the respective asset.
2. Borrow an amount such that contract has sufficient liquid tokens to pay order outputs as orders
   are processed and settled 1-by-1.

### `settle`

The `settle` amount is the total in liquid tokens to be paid to Uniswap to repay borrows as well as
pay for the input side of pool swaps.

### Example

Imagine a bundle with:
1. AMM Swap A -> B, pool receives 300 A, angstrom receives 1200 B
2. User order A -> B, user pays 1100 A, receives 4000 B (fee: 100 A)
3. User order B -> A, user pays 2800 B, receives 650 A (fee: 50 A)

Asset list (assuming contract's A balance is already 1200):

```yaml
-
    asset: A
    save: 150
    take: 0
    settle: 300
-
    asset: B
    save: 0
    take: 2800
    settle: 1600
```

**Explanation:**

For A: Extra fees are always charged in `asset0` of the pair which is assigned to A's `take` (50 + 100 =
150). The first order we process gives us all the `A` liquidity we need so we don't need to `take`
anything. Due to the AMM Swap we owe Uniswap 300 A so we set `settle` to 300.

For B: We need 4000 B to pay the first order but we already have 1200 in the contract so we only
need to borrow 2800. Due to the AMM swap we virtually receive 1200 B so we only owe Uniswap 1600
(2800 - 1200) requiring us to set `settle` to 1600.

## Pair list

For each pair the node needs to look up the `store_index` for the `(asset0, asset1)` pair as well as
determine a uniform clearing price (`price_1over0`) for that pair. This should be the output of the
order matching algorithm.

## Pool Updates

The pool updates contain information about swaps to perform against the underlying Uniswap pool and
what rewards to distribute to that pool. **Note** it is a requirement that no one else, except for
the Angstrom contract should be able to swap against the pools. This is to protect the LPs from
outside searchers who may seek to extract the arbitrage value for themselves.

Per pair it is expected that one swap will be executed based on the winning ToB order. The swap's
input/output should be set such that the ToB order's output is filled. The positive difference ToB
input and required swap input is taken as the ToB order's "bid". This is to be distributed, along
with a share of trading fees to LPs.

The ticks that were used to complete the swap should be the ones to receive the reward, how and in
what proportions is to be specified elsewhere. The reward distribution is then specified via the
`RewardsUpdate`.

User orders can also be matched against the pool's liquidity based on the post-ToB swap price. In
that case the total user order swap + ToB swap can be netted out into a single swap in the
`PoolUpdate`.

If for some reason ticks on both sides of the final tick need to be rewarded two `PoolUpdate` may be
added for a single pool. For the sake of gas the number of pool updates per pair is not enforced.

## Orders

### ToB Orders

Only 1 ToB order should be supplied per pair but this also is not checked as it would not give any
additional guarantees.

### Gas / Extra fee

The node should attribute each order's share of the gas cost via the `gas_used_asset0` &
`extra_fee_aset0` fields. Additionally `extra_fee_asset0` may include referral fees for user orders.
