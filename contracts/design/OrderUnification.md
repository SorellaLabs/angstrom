# Intents Compression

## Status Quo

Here are the current intents structs:

```solidity
struct LimitOrder {
    Currency currencyIn;
    Currency currencyOut;
    address amountIn;
    address amountOut;
    uint256 gasCap;
    bool allowPartial;
    uint256 deadline;
    bytes preHook;
    bytes postHook;
}

struct SearcherOrder {
    Currency currencyIn;
    Currency currencyOut;
    address amountIn;
    address amountOut;
    uint256 bribe;
    uint256 gasCap;
    uint256 deadline;
    bytes preHook;
    bytes postHook;
}

struct UserOrder {
    Currency currencyIn;
    Currency currencyOut;
    uint128 amountIn;
    uint128 amountOutMin;
    uint256 gasCap;
    uint256 deadline;
    bytes preHook;
    bytes postHook;
}
```

For searcher and user orders, here are how their fields are used:

```solidity
function _processSearcherIn(LvrSettlement calldata aLvrBid) private returns (address rOwner) {
    SearcherOrder calldata lOrder = aLvrBid.order;

    // Recover owner & enforce constraints.
    rOwner = _recoverSigner(lOrder.structHash(), aLvrBid.signature);
    require(block.timestamp <= lOrder.deadline, "LVR: Deadline exceeded");
    require(aLvrBid.gasActual <= lOrder.gasCap, "LVR: Gas cap exceeded");

    // Run pre hook if one exists.
    if (lOrder.preHook.length > 0) {
        _executeHook(lOrder.preHook);
    }

    // Transfer amountIn, bribe, and gasBid to this contract.
    IERC20(Currency.unwrap(lOrder.currencyIn)).transferFrom(
        rOwner, address(this), lOrder.amountIn + lOrder.bribe + aLvrBid.gasActual
    );
}

function _processUserIn(UserSettlement calldata aUser) private returns (address rOwner) {
    UserOrder calldata lOrder = aUser.order;

    // Recover owner and enforce constraints.
    rOwner = _recoverSigner(aUser.order.structHash(), aUser.signature);
    require(block.timestamp <= lOrder.deadline, "USER: Deadline exceeded");
    require(aUser.amountOut >= lOrder.amountOutMin, "USER: Insufficient output");
    require(aUser.gasActual <= lOrder.gasCap, "USER: Gas cap exceeded");

    // Run pre hook if one exists.
    if (lOrder.preHook.length > 0) {
        _executeHook(lOrder.preHook);
    }

    // Retrieve amountIn + gas from user.
    IERC20(Currency.unwrap(lOrder.currencyIn)).transferFrom(
        rOwner, address(this), lOrder.amountIn + aUser.gasActual
    );
}
```

The following constraints **ARE** enforced by the contract:

- The searcher/user's `amountIn` does not exceed their signed `amountIn`.
- The searcher/user's `amountOut` is not below their signed `amountOut{,Min}`.
- The searcher/user's deadline has not been exceeded.
- The searcher/user's pre/post hooks are executed.

The following constraints **ARE NOT** enforced by the contract:

- The searcher/user's got the best price possible, this is not provable on-chain
  and thus the guard's are trusted to act fairly.

## Proposed New Struct

Without violating any constraints, we can service all order types in a single
unified struct. This will simplify the protocol and most likely reduce gas
costs.

```solidity
struct Order {
    Currency currencyIn;
    Currency currencyOut;
    uint128 amountIn;
    uint128 amountOutMin;
    bool allowPartial;
    uint256 deadline;
    bytes preHook;
    bytes postHook;
}
```

In this one struct, all prior order types can be expressed, see examples:

```solidity
Order memory limitOrder = Order({
    currencyIn: legacy.currencyIn,
    currencyOut: legacy.currencyOut,
    amountIn: legacy.amountIn + legacy.gasCap,
    amountOutMin: legacy.amountOut,
    allowPartial: true,
    deadline: legacy.deadline,
    preHook: bytes(""),
    postHook: bytes("")
});

Order memory searcherOrder = Order({
    currencyIn: legacy.currencyIn,
    currencyOut: legacy.currencyOut,
    amountIn: legacy.amountIn + legacy.gasCap + legacy.bribe,
    amountOutMin: legacy.amountOut,
    allowPartial: false,
    deadline: legacy.deadline,
    preHook: legacy.preHook,
    postHook: legacy.postHook
});

Order memory userOrder = Order({
    currencyIn: legacy.currencyIn,
    currencyOut: legacy.currencyOut,
    amountIn: legacy.amountIn + legacy.gasCap,
    amountOutMin: legacy.amountOut,
    allowPartial: false,
    deadline: legacy.deadline,
    preHook: legacy.preHook,
    postHook: legacy.postHook
});
```

Due to similarities between actors intents, all legacy orders can be represented
in the new structure without any observable side-effects.

## Disclaimer

It is assumed that gasCap is proportional when `allowPartial: true`, this is
because if it was not proportional then a user could not guarantee their limit
price (if they trade 10% of requested but pay full gas, their price may exceed
their limit).
