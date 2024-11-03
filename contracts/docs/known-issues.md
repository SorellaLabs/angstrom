# Known Issues
- `PoolConfigStore` does not check that `tickSpacing < MAX`
- `_decodeAndReward` may DoS if trying to reward far out ticks => not a major attack as reward
algorithm is *generally* correlated to ticks swapped across
- pair fee (`feeInE6`) rounds in favor of the protocol i.e. fee could be more than the configured
percentage *if* the fee to be charged is a fractional amount. Then it's rounded up to the nearest
base unit.
- decoding can devolve into infinite loop resulting in OOG error if incorrectly encoded
- can only encode up to `N` of `X` => limited count of assets, pairs and orders is intentional 
- can only specify output for partial orders
- can add liquidity to disabled pools
- can only configure 1 Uniswap AMM pool at a time
- no events: we avoid events to save gas and also to simplify the later fee summary event
co-processing logic
- use of custom `controller` auth logic instead of standard `Ownable`: standard `Ownable` typically
  tracks events as well as a redundant `renounceOwnership` function which we do not need for our use
  case.

## Bundle Building Footguns
- Liquidity positions do not include the upper bound, donating to the upper bound will not credit
positions which do not otherwise encompass that tick and may even burn the tokens if you donate to
the highest uninitialized bound.
- Can also burn funds by donating to the current tick when liquidity is zero.

