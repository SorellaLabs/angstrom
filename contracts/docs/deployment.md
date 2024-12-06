# Deployment

Copy `.env.example` into a `.env` file and then set the following environment variables:

- `ANGSTROM_MULTISIG`: Address of the Angstrom multisig wallet that will have proposal/cancel permissions for the timelock
- `ANGSTROM_ADDRESS_TOKEN_ID`: Sub Zero Token ID for Angstrom's vanity address
- `V4_POOL_MANAGER`: Address of the Uniswap V4 pool manager contract

Then run `forge script script/Angstrom.s.sol:AngstromScript --rpc-url <ETH_RPC_URL>` to simulate
the deployment.

Add `--broadcast` and one of the wallet options to trigger an actual deployment.
