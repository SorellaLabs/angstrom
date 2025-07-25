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

Amortized cost of `N` orders not including the ToB order cost (~ f40kor liquid balance ToB). When
with AMM the donate is a `CurrentOnly` donate with a non-zero amount.

- EFI = Exact Flash Order \w Internal Balances
- ESLn = Exact Standing Order \w Liquid Tokens (Nonce non-zero)

| Order Count | EFI (\w AMM) | EFI (No AMM) | ESLn (\w AMM) | ESLn (No AMM) |
| ----------- | ------------ | ------------ | ------------- | ------------- |
| 1           | 148.9k       | 70.4k        | 159.0k        | 80.7k         |
| 2           | 84.2k        | 44.9k        | 95.7k         | 56.5k         |
| 3           | 62.6k        | 36.4k        | 74.6k         | 48.4k         |
| 4           | 51.8k        | 32.2k        | 64.0k         | 44.4k         |
| 5           | 45.3k        | 29.6k        | 57.7k         | 42.0k         |
| 10          | 32.4k        | 24.5k        | 45.0k         | 37.2k         |
| 20          | 25.9k        | 22.0k        | 38.7k         | 34.8k         |
| 50          | 22.0k        | 20.4k        | 34.9k         | 33.3k         |

## Uni Benchmarks

Test uni v4 direct swap: 123,144
V4Router_ExactInputSingle: 134,001
