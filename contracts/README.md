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

Amortized cost of `N` orders not including the ToB order cost (~34.1k for liquid balance ToB). When
with AMM the donate is a `CurrentOnly` donate with a non-zero amount.

- EFI = Exact Flash Order \w Internal Balances
- ESLn = Exact Standing Order \w Liquid Tokens (Nonce non-zero)

|Order Count|EFI (\w AMM)|EFI (No AMM)|ESLn (\w AMM)|ESLn (No AMM)|
|-----------|------------|------------|-------------|-------------|
| 1| 156.0k | 77.5k | 166.2k | 87.8k |
| 2| 87.7k | 48.5k | 99.2k | 60.1k |
| 3| 65.0k | 38.8k | 76.9k | 50.8k |
| 4| 53.6k | 34.0k | 65.8k | 46.2k |
| 5| 46.7k | 31.1k | 59.1k | 43.4k |
|10| 33.1k | 25.2k | 45.7k | 37.9k |
|20| 26.3k | 22.3k | 39.0k | 35.1k |
|50| 22.2k | 20.6k | 35.0k | 33.4k |
