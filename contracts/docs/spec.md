# Specification

## Units

- `WAD`: `1e18` (uint256)
- `RAY`: `1e27` (uint256)

## Bundle

**Required Data**


- top-level bundle data
    - main asset list: sorted array of all addresses (including `address(0)` for NATIVE) involved in
      the bundle (the **asset index** refers to an assets index in this list)
    - uniform clearing prices list: list of tuples `(i, j, price)` where `i` and `j` are the indices
      of the assets, `i > j` and `price` is the price of `i` vs. `j` as fixed-point "RAY" number
      (10^27).
    - submitting guardian (propser / builder) ECDSA signature (compressed with [EIP-2098](https://eips.ethereum.org/EIPS/eip-2098))
- list of ToB trades
- list of operations, each operation is broadly either:
    - normal trade execution (one of the 4 normal trade types)
    - execute pool swap
    - a settlement action (send tokens to users, draw liquidity from uniswap) 
        - user-to-user ERC20 transfer
        - user-to-user Uniswap V4 claim transfer
        - transfer ERC20 tokens from user to contract
        - transfer asset from contract to user
        - transfer asset to user via Uniswap V4 Delta
        - transfer ERC20 tokens from user to Uniswap V4
        - transfer asset to contract via Uniswap V4 Delta
        - transfer asset from contract to Uniswap V4
        - settle Uniswap V4 delta with available tokens


## Order Structs
### Base Types

```solidity
enum AssetForm {
    Liquid,
    UniV4Claim,
    AngstromClaim
}

enum OrderType {
    Bid_AtomicExactIn,   // 0
    Ask_AtomicExactIn,   // 1
    Bid_AtomicExactOut,  // 2
    Ask_AtomicExactOut,  // 3
    Bid_Partially,       // 4
    Ask_Partially        // 5
}
```

> [!NOTE]
> The `OrderType` enum is interpreted as an 8-bit uint by EIP712. The ordering of the variants
> ensures that the lower bit of the variant index indicates Bid/Ask as well as having a cut-off
> value for atomic vs. partially fillable orders.

### Top-of-Block (ToB) Trade

ToB trades only have two variants, composable and non-composable.


> [!NOTE]
> The `amount` parameter is signed integer as it will be fed direclty as the input to the V4 swap
> function, meaning that negative numbers indicate an exact input amount and positive an exact
> output.


```solidity
struct NonComposableTopOfBlockTrade {
    bool zeroForOne;
    uint256 amountIn;
    uint256 minAmountOut;
    uint256 bid;
    address asset0;
    AssetForm asset0Form;
    address asset1;
    AssetForm asset1Form;
    uint256 validBlockNumber;
}

struct ComposableTopOfBlockTrade {
    bool zeroForOne;
    uint256 amountIn;
    uint256 minAmountOut;
    uint256 bid;
    address asset0;
    AssetForm asset0Form;
    address asset1;
    AssetForm asset1Form;
    uint256 validBlockNumber;
    bytes hookPayload;
}
```

### Shared Normal Order Body

All normal order types (composable/non-composable, flash/standing) will hold at least these
attributes:

```solidity
struct CommonOrderBody {
    OrderType otype;
    uint256 amount;
    uint256 limitPrice;
    address assetIn;
    AssetForm assetInForm;
    address assetOut;
    AssetForm assetOutForm;
}
```

### Order Type Specific Fields

**Flash Order**
```solidity
struct FlashOrderFooter {
    uint256 validBlockNumber;
}
```

**Standing Order**
```solidity
struct StandingOrderFooter {
    uint64 nonce;
    uint256 deadline;
}
```

**Composable Orders**

```solidity
struct ComposableOrderFooter {
    bytes hookPayload;
}
```

**Non-Composable Orders**

```solidity
struct NonComposableOrderFooter {
    // A non-composable order does not carry any extra fields
}
```

### Full Normal Orders

The normal order types are composed as:

```
struct {NonComposable/Composable}{Flash/Standing}Order {
    // Normal body.
    OrderType otype;
    uint256 amount;
    uint256 limitPrice;
    address assetIn;
    AssetForm assetInForm;
    address assetOut;
    AssetForm assetOutForm;
    // Added attributes based on the sub-type (e.g. composable standing)
}
```

This means there are a total of 4 order types:


```solidity
struct NonComposableFlashOrder {
    // Normal body.
    OrderType otype;
    uint256 amount;
    uint256 limitPrice;
    address assetIn;
    AssetForm assetInForm;
    address assetOut;
    AssetForm assetOutForm;
    // Extra attributes.
    uint256 validBlockNumber;
}

struct NonComposableStandingOrder {
    // Normal body.
    OrderType otype;
    uint256 amount;
    uint256 limitPrice;
    address assetIn;
    AssetForm assetInForm;
    address assetOut;
    AssetForm assetOutForm;
    // Extra attributes.
    uint64 nonce;
    uint256 deadline;
}

struct ComposableFlashOrder {
    // Normal body.
    OrderType otype;
    uint256 amount;
    uint256 limitPrice;
    address assetIn;
    AssetForm assetInForm;
    address assetOut;
    AssetForm assetOutForm;
    // Extra attributes.
    uint256 validBlockNumber;
    bytes hookPayload;
}

struct ComposableStandingOrder {
    // Normal body.
    OrderType otype;
    uint256 amount;
    uint256 limitPrice;
    address assetIn;
    AssetForm assetInForm;
    address assetOut;
    AssetForm assetOutForm;
    // Extra attributes.
    uint64 nonce;
    uint256 deadline;
    bytes hookPayload;
}
```
