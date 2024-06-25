# ERC1155 Explainer

This document highlights how Angstrom interfaces with ERC1155 tokens issued by
UniswapV4's PoolManger contract.

## Uniswap V4 Pool Manager

### Mint ERC1155

If a user wishes to store the output of a trade within UniswapV4, they would
mint an ERC1155 which acts as a claim on the underlying:

```solidity
function mint(Currency currency, address to, uint256 amount) external override noDelegateCall onlyByLocker {
    _accountDelta(currency, amount.toInt128());
    _mint(to, currency.toId(), amount, "");
}
```

### Burn ERC1155 to trade/redeem

To later withdraw or trade with this claim they must "transfer" the token to the
PoolManager which triggers a burn of the token (crediting the underlying for use
in `swap` or `settle` calls).

```solidity
function onERC1155Received(address, address, uint256 id, uint256 value, bytes calldata) external returns (bytes4) {
    if (msg.sender != address(this)) revert NotPoolManagerToken();
    _burnAndAccount(CurrencyLibrary.fromId(id), value);
    return IERC1155Receiver.onERC1155Received.selector;
}
```

## Angstrom

## Burn user ERC1155 for input

If a user wishes to trade with a stored ERC1155, they will need to approve
Angstrom as a spender so we can transfer the token to the PoolManager. This
transfer will trigger the fallback logic that burns the holding an credits the
underlying for trading.

```solidity
if (lOrder.burnIn) {
    IERC1155(Currency.unwrap(lOrder.currencyIn)).safeTransferFrom(
        rOwner, address(poolManager), lOrder.currencyIn.toId(), lOrder.amountIn, bytes("")
    );
} else {
    IERC20(Currency.unwrap(lOrder.currencyIn)).transferFrom(rOwner, address(this), lOrder.amountIn);
}
```

## Mint user ERC1155 for output

If a user wishes to store/leave their output in the PoolManager, they must set
a flag in their order. This will trigger a `mint` instead of the usual
`ERC20.transfer`.

```solidity
if (lOrder.mintOut) {
    poolManager.mint(lOrder.currencyOut, aOwner, aExecution.amountOutActual);
} else {
    lOrder.currencyOut.transfer(aOwner, aExecution.amountOutActual);
}
```

## Handing native ETH

A user can convert native ETH to UniswapV4 ERC1155 ETH by transferring ETH to
the PoolManager contract and then calling `settle`.

```solidity
function settle(Currency currency) external payable override noDelegateCall onlyByLocker returns (uint256 paid) {
    uint256 reservesBefore = reservesOf[currency];
    reservesOf[currency] = currency.balanceOfSelf();
    paid = reservesOf[currency] - reservesBefore;
    // subtraction must be safe
    _accountDelta(currency, -(paid.toInt128()));
}

receive() external payable {}
```

This ERC1155 then behaves like any other ERC1155. When you redeem it by burning
and calling `take` you will get native ETH back.
