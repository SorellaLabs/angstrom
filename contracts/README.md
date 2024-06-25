# Angstrom

This repository contains the core contracts for the Angstrom protocol. These
contracts enforce decisions made by the off-chain network.

## TODOs

- [ ] Angstrom Core
    - [x] Limit orders
        - [x] Order validation
        - [x] Order accounting (execution)
        - [x] Order settlement
        - [x] Charge swap fees
    - [ ] Uniswap V4
        - [x] Base actions (settle delta, acquire lock)
        - [x] Pool swaps
        - [ ] 🚧 Account tick-based rewards (fees, bids, uniform clearing comp)
        - [ ] Disperse rewards on hook
    - [ ] Gas accounting
        - [ ] Design gas management (solve "denomination problem")
        - [ ] Add fields for specifying gas
        - [ ] Charge and account for gas
    - [ ] Consensus
        - [x] Node <> ETH Account mapping
        - [ ] Node BLS key mapping
        - [ ] Node reward dispersal
            - [ ] Design method for canceling out denominations
            - [ ] Integrate BLS signature validation library
            - [ ] Accept dispersal claim submission from consensus
        - [ ] Eigenlayer based registration
            - [ ] Governance controlled whitelist
            - [ ] Checks Eigenlayer staking registry and records share
            - [ ] Pushes update to contract
- [ ] Testing

