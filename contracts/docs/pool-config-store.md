# Pool Configuration & Pool Config Store

Every valid Angstrom pair is configured to work with 1 underlying uniswap pool. The term "pair" and
"pool" may be used interchangeably in these docs.

The parameters of a pool are:
- `tick_spacing: uint16` - The `tickSpacing` parameter of the underlying Uniswap V4 pool
- `fee_in_e6: uint24` - The fee for limit orders in that pair in 0.000001 % (1e-6). Capped to 20%.

Pool parameters are changeable after initial configuration. The underlying Uniswap pool must be
initialized separately once a new set of parameters is set.

## Pool Config Store

To minimize the gas cost of looking up & validating these parameters when processing an Angstrom
bundle they're stored in a "store contract", this is a conract that holds the data as its raw
bytecode (padded with one leading `00` byte to prevent execution/destruction).

The store's bytecode is structured as follows:
```
store_bytecode = safety_byte config_entry+
safety_byte = 0x00
config_entry = pool_partial_key tick_spacing fee_in_e6
```

### Store Key

[Solidity implementation](../src/libraries/PoolConfigStore.sol)

Pools in the store are uniquely identified by their store key. The store key is derived by
hashing the sorted `(asset0, asset1)` and then truncating the upper 5 bytes (such that every
`config_entry` is 27 bytes).

### Store Index

The "store index" is the 0-indexed entry offset in the store e.g. if there are 5 entries total and
the pool is the 3rd, its store index is `2`.
