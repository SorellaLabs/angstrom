## 28. June

**Improved `TestnetHub`, renamed to `PoolGate`**

Replaced `TestnetHub` with `PoolGate` in testnet deployment now has simplified interface. You no
longer need to ABI encode calls and pass them to `execute`. Instead you can directly call:
- `initializePool(address asset0, address asset1, uint160 initialSqrtPriceX96)` (initialized a pool
for the configured hook given an `asset0`, `asset1` and start price `initialSqrtPriceX96`)
- `addLiquidity(address asset0, address asset1, int24 tickLower, int24 tickUpper, uint256 liquidity)` (adds liquidity)
