# LP Handling

## Problem

UniswapV4 contracts only allow the owner of the position to modify the position.
This can be seen best in this code snippet:

```solidity
(delta, feeAmounts) = pools[id].modifyPosition(
    Pool.ModifyPositionParams({
        owner: msg.sender,
        tickLower: params.tickLower,
        tickUpper: params.tickUpper,
        liquidityDelta: params.liquidityDelta.toInt128(),
        tickSpacing: key.tickSpacing
    })
);
```

`owner` is forced to be `msg.sender`, meaning that EOAs cannot delegate position
management logic to smart contracts. This prevents Angstrom DEX from modifying a
user's position based off of a signed intent.

## Solution

There are various possible solutions, I will enumerate some here.

### Uniswap adds `approveModifyPosition(address modifier)`

This would be similar to an infinite approval in the ERC20 world. However, it
would cover all positions including those that are not part of the Angstrom
ecosystem.

The assumption is that most `modifiers` will only be smart contracts and that
these smart contracts should not be upgradable and would take user custody
seriously.

Users would only call approval once for each smart contract they wish to provide
management privileges to. If they wish to firewall protocols they can use
additional addresses.

The estimated overhead for this solution is:

- Extra `JUMPI` for existing use-cases.
  - Need to check is `msg.sender == positionOwner` and branch based on that.
- Cold `SLOAD` when position modifiers operate on a user's behalf.
  - Load from `mapping(address => mapping(address => bool))` if
    `msg.sender != positionOwner`.

This solution is recommended.

### Uniswap adds `approveModifyPosition(address modifier, PositionKey key)`

This is similar to approving a user to blanket modify any positions, however, it
would be only for a single position. The main downside is that most positions
are single use and thus would all incur the approval overhead.

Users would also have to approve before creating, meaning if the creation fails
they still pay for the approval.

This solution is not recommended.

### Uniswap adds `permitModify(address owner, address modifier, bytes sig)`

This would be the transient alternative to `approveModifyPosition`. This would
require LPs to sign two messages:

1. Approval to modify their position.
2. Signed LP intent for batching in Angstrom.

Furthermore, this scheme does not support smart contracts/wallets, as they would
require something like ERC1271 support for this to work.

The overhead of this scheme is:

- 3000 GAS for `ECRECOVER`.
- Additional signature calldata overhead.

This solution is not recommended.

### Angstrom creates `Identity` contracts for each user

Angstrom could `CREATE2` a unique identity the first time each user interacts
with the Angstrom DEX. This identity would be derived from the the user's
address, meaning no `SSTORE` or `SLOAD`s would be necessary.

The estimated overhead for this solution is:

- ~80k GAS on first interaction to:
  - Deploy `Identity` contract for user.
- ~7k GAS to:
  - Call the `Identity` contract with modify arguments.
  - Have the `Identity` contract call `poolManager.lock`.
  - Have the `Identity` contract call `poolManager.modifyPosition`.
  - Have the `Identity` contract call `angstrom.payManager`.
  - Potential extraneous ERC-20 transfer that could have been avoided with net
    settlement.
  - Have the `Identity` contract call `poolManager.settle` or `poolManager.take`
    twice.

This solution is recommended if UniswapV4 does not adjust their design.

### Users call `UniswapV4` directly

Angstrom could store a flag indicating the block of the most recent batch. This
would determine whether LPs could modify their positions. LPs would send gasless
transactions to modify their positions to ensure inclusion only if they are
after the batch.

It is unclear if we would need the last batch flag for other reasons. If we do
not then this solution would have the following overhead:

- For trade batches:
  - 2900 GAS to `SSTORE` the last batch number.
- For LPs:
  - 2100 GAS to `SLOAD` the last batch number.
