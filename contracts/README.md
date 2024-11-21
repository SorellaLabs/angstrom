# Angstrom

This repository contains the core contracts for the Angstrom protocol. These
contracts enforce decisions made by the off-chain network.

For docs see [./docs](./docs/).


## Build Instructions

1. Ensure you have the foundry toolchain installed (otherwise get it from `https://getfoundry.sh/`)
2. Run `forge build`
3. Setup a python virtual environment under `.venv` (using uv: `uv venv .venv`)
4. Ensure the python packages from `requirements.txt` are installed into the environment (`source .venv/bin/activate && uv pip install -r requirements.txt`)
5. Run tests with `forge test --ffi`

### Alternative Python Environment
If you do not have Python 3.12 or simply want to use your global installation instead of a virtual
environment you can tweak what python executable is used for the FFI tests by:
1. Opening [`test/_helpers/BaseTest.sol`](./test/_helpers/BaseTest.sol)
2. Changing `args[0]` in `pythonRunCmd()` to a different path e.g.

```diff
function pythonRunCmd() internal pure returns (string[] memory args) {
    args = new string[](1);
--  args[0] = ".venv/bin/python3.12";
++  args[0] = "python3";
}
```

## Benchmark Numbers

### Total Cost

Cost of `N` orders including the fixed cost.

- EFI = Exact Flash Order \w Internal Balances
- ESLn = Exact Standing Order \w Liquid Tokens (Nonce non-zero)

|Order Count|EFI (\w AMM)|EFI (No AMM)|ESLn (\w AMM)|ESLn (No AMM)|
|-----------|------------|------------|-------------|-------------|
| 1|  137.2k |   65.1k |  150.2k |   92.7k |
| 2|   78.3k |   42.2k |   91.3k |   62.6k |
| 3|   58.7k |   34.6k |   71.7k |   52.5k |
| 4|   48.8k |   30.8k |   61.9k |   47.5k |
| 5|   43.0k |   28.5k |   56.0k |   44.5k |
|10|   31.2k |   24.0k |   44.2k |   38.4k |
|20|   25.3k |   21.7k |   38.3k |   35.4k |
|50|   21.8k |   20.3k |   34.8k |   33.6k |

**Real Bundle Cost per Exact Flash Order \w Internal Balance**

Fixed Cost (including calldata + intrinsic, not including ToB & pool updates): 45.7k
Cost per order: 19.4k

|Order Count|Total amortized per order cost|
|-----------|------------------------------|
|2| 42.2k|
|3| 34.6k|
|4| 30.8k|
|5| 28.5k|
|10| 24.0k|
|20| 21.7k|
|50| 20.3k|

**Real Bundle Cost per Exact Standing Order \w Liquid Tokens (Nonce non-zero)**

Fixed Cost (including calldata + intrinsic, not including ToB & pool updates): 45.7k
Fixed Cost per unique ERC20 token: 7,300
Cost per order: 32.4k

|Order Count|Total amortized per order cost|
|-----------|------------------------------|
|2|  62.5k|
|3|  52.5k|
|4|  47.5k|
|5|  44.5k|
|10| 38.4k|
|20| 35.4k|
|50| 33.6k|
