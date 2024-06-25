# Naive Guard Consensus

This file outlines a naive consensus algorithm that could be used for the
off-chain guard network. It is constructed with the current contracts in mind.

## Auction Timeline

Let's assume the following auction timeline:

| Time | State                       |
| ---  | ---                         |
| T-12 | ETH2 Slot Start             |
| T-2  | No new searcher/user orders |
| T-1  | Bundle selected             |

The key challenge is how does one quickly reach consensus on which bundle should
be submitted on-chain. We assume some single-round signature scheme exists that
allows for quick proving of a 2/3 majority.

## Assumptions

### Guard Behavior

- Guards are directly peered with all other guards.
- Honest guards continuously gossip user & searcher orders.

### Bundle Selection Algorithm

- There is a deterministic way to determine if a bundle is valid or invalid.
- There is something akin to a "reward function" that allows for absolute
  ordering of all bundles.

## Algorithm

1. Round timer elapses and each guard signs the best bundle they have seen.
2. All guards send their singular best signed bundles to all other guards.
3. Guards wait for some liveness timeout and then sort all signed bundles,
   picking the one with the highest score according to the "reward function".
   The guards sign their new selection and if 2/3 sign the same choice we will
   have consensus (assuming BLS or some offline signature aggregation scheme).

## Failure Modes

| Threshold | Consequence                                          |
| ---       | ---                                                  |
| 1/3       | Prevent/delay bundle inclusion                       |
| 2/3       | Sign bundles that steal LVR & user limit price delta |

It is assumed that in a 1/3 failure bundle creation would halt until 1/3 was
slashed/rotated out or became active again.

## Open Problem

This construction's critical weakness is the assumption that one can select a
"liveness timeout" that allows all honest actors to co-ordinate properly. If
this timeout proves to short, the construction could halt even with onl honest
actors.

Without some "online" mechanism to produce the votes, there will be a risk of
duplicate valid bundles for the same slot if we re-run the auction.
Additionally, latency will increase the longer we take to finalize the bundle.

Consensus algorithms solve this by running multiple rounds and using a fork
selection rule to ensure the honest majority will eventually triumph.

Perhaps MPC can solve the problem of duplicate bundle production by make the
signature process atomic (either 2/3 is reached and the bundle locks in, or it
is not an all intermediate sigs are unusable).
