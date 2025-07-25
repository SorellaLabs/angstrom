import json
import os

CURRENT_PATH = os.path.dirname(__file__)

with open(os.path.join(CURRENT_PATH, '../snapshots/Full Bundle Benchmark.json')) as f:
    raw_nums = json.load(f)

raw_nums = {
    k: int(v)
    for k, v in raw_nums.items()
}


tob_cost = 39236 + 1000

# Measurements from `../test/benchmark/FullBundle.b.sol` (with --isolate --flamechart `.execute`)

# efi = Exact Flash order, Internal balances
efi_amm_total_1 = raw_nums['test_exactFlashInternal_amm_1']
efi_amm_total_2 = raw_nums['test_exactFlashInternal_amm_2']
efi_amm_total_3 = raw_nums['test_exactFlashInternal_amm_3']
efi_amm_var = efi_amm_total_2 - efi_amm_total_1
efi_amm_var2 = efi_amm_total_3 - efi_amm_total_2
print(f'efi_amm_var: {efi_amm_var} ({efi_amm_var2 / efi_amm_var - 1:.2%})')
efi_amm_fixed = efi_amm_total_1 - efi_amm_var

efi_solo_total_1 = raw_nums['test_exactFlashInternal_solo_1']
efi_solo_total_2 = raw_nums['test_exactFlashInternal_solo_2']
efi_solo_total_3 = raw_nums['test_exactFlashInternal_solo_3']
efi_solo_var = efi_solo_total_2 - efi_solo_total_1
efi_solo_var2 = efi_solo_total_3 - efi_solo_total_2
print(f'efi_solo_var: {efi_solo_var} ({efi_solo_var2 / efi_solo_var - 1:.2%})')
efi_solo_fixed = efi_solo_total_1 - efi_solo_var

# esln = Exact Standing order, Liquid token balances, Non-zero starting nonce
esln_amm_total_1 = raw_nums['test_exactStandingLiquidNonZeroNonce_amm_1']
esln_amm_total_2 = raw_nums['test_exactStandingLiquidNonZeroNonce_amm_2']
esln_amm_total_3 = raw_nums['test_exactStandingLiquidNonZeroNonce_amm_3']
esln_amm_var = esln_amm_total_2 - esln_amm_total_1
esln_amm_var2 = esln_amm_total_3 - esln_amm_total_2
print(f'esln_amm_var: {esln_amm_var} ({esln_amm_var2 / esln_amm_var - 1:.2%})')
esln_amm_fixed = esln_amm_total_1 - esln_amm_var

esln_solo_total_1 = raw_nums['test_exactStandingLiquidNonZeroNonce_solo_1']
esln_solo_total_2 = raw_nums['test_exactStandingLiquidNonZeroNonce_solo_2']
esln_solo_total_3 = raw_nums['test_exactStandingLiquidNonZeroNonce_solo_3']
esln_solo_var = esln_solo_total_2 - esln_solo_total_1
esln_solo_var2 = esln_solo_total_3 - esln_solo_total_2
print(
    f'esln_solo_var: {esln_solo_var} ({esln_solo_var2 / esln_solo_var - 1:.2%})')
esln_solo_fixed = esln_solo_total_1 - esln_solo_var

v3_gas = 140_000


def fmt(x: float) -> str:
    # multip = x / v3_gas
    # return f'{multip:7.1%}'
    return f'{x / 1e3:.1f}k'


adjustment = tob_cost

for i in (1, 2, 3, 4, 5, 10, 20, 50):
    efi_amm = efi_amm_var + (efi_amm_fixed - adjustment) / i
    efi_solo = efi_solo_var + (efi_solo_fixed - adjustment) / i
    esln_amm = esln_amm_var + (esln_amm_fixed - adjustment) / i
    esln_solo = esln_solo_var + (esln_solo_fixed - adjustment) / i
    print(f'|{i:2}| {fmt(efi_amm)} | {fmt(efi_solo)} | {fmt(esln_amm)} | {fmt(esln_solo)} |')
